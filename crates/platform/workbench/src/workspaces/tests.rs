use std::path::PathBuf;

use serde_json::{Value, json};
use zen_core::{Registry, Session};

use super::{WorkspaceInfo, Workspaces};
use crate::Workbench;

struct Window {
    registry: Registry,
    session: Session,
    dir: tempfile::TempDir,
}

impl Window {
    fn new() -> Self {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(project.join("a.go"), "package a\n").unwrap();
        std::fs::write(project.join("b.go"), "package b\n").unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
        let workspaces = session.state_mut::<Workspaces>();
        workspaces.set_data_dir(dir.path().join("data"));
        workspaces.register(WorkspaceInfo { id: "test".into(), title: "Test".into(), icon: "database".into() });
        let mut window = Self { registry, session, dir };
        window.run("file.document.open", json!({ "path": window.project("a.go") }));
        window.run("file.document.open", json!({ "path": window.project("b.go") }));
        window
    }

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

    fn refused(&mut self, command: &str, params: Value) -> String {
        self.registry.dispatch(&mut self.session, command, params).unwrap_err().to_string()
    }

    fn state(&mut self) -> Value {
        self.run("workspace.host.state", json!({ "id": "test" }))
    }

    fn focused(&self) -> u64 {
        self.session.state::<Workbench>().unwrap().groups().focused()
    }

    fn active_path(&self) -> Option<PathBuf> {
        self.session.state::<Workbench>().unwrap().active_document().map(|document| document.path().to_owned())
    }

    fn tab_titles(&mut self) -> Vec<String> {
        let state = self.state();
        state["tabs"].as_array().unwrap().iter().map(|tab| tab["title"].as_str().unwrap().to_owned()).collect()
    }
}

#[test]
fn expanded_is_the_focus_of_the_owned_group_whatever_moves_it() {
    let mut window = Window::new();
    let listed = window.run("workspace.host.list", json!({}));
    assert_eq!(listed["workspaces"][0]["id"], "test");
    assert_eq!(listed["expanded"], Value::Null);

    window.run("editor.split.right", json!({}));
    window.run("editor.split.focus", json!({ "group": 1 }));
    let state = window.run("workspace.host.expand", json!({ "id": "test" }));
    let group = state["group"].as_u64().unwrap();
    assert_eq!(state["expanded"], true);
    assert_eq!(state["returnTo"], 1, "the main group that had the keyboard");
    assert_eq!(window.focused(), group, "expanding focuses the owned group");
    assert_eq!(window.active_path(), None, "an empty workspace shows no document");

    let listed = window.run("workspace.host.collapse", json!({}));
    assert_eq!(listed["expanded"], Value::Null);
    assert_eq!(window.focused(), 1, "collapsing focuses the remembered main group");
    assert_eq!(window.active_path(), Some(window.project("b.go")));

    window.run("workspace.host.toggle", json!({ "id": "test" }));
    assert_eq!(window.state()["expanded"], true);
    window.run("editor.split.focus", json!({ "group": 2 }));
    assert_eq!(window.state()["expanded"], false, "focusing a main group collapses");
    window.run("workspace.host.toggle", json!({ "id": "test" }));
    assert_eq!(window.state()["returnTo"], 2);
    window.run("editor.split.next", json!({}));
    assert_eq!(window.state()["expanded"], false, "the next splitter collapses");
    window.run("workspace.host.expand", json!({ "id": "test" }));
    window.run("file.document.open", json!({ "path": window.project("a.go") }));
    assert_eq!(window.state()["expanded"], false, "opening a code file collapses");
    window.run("workspace.host.toggle", json!({ "id": "test" }));
    window.run("workspace.host.toggle", json!({ "id": "test" }));
    assert_eq!(window.state()["expanded"], false, "toggle twice is back");

    window.run("workspace.host.collapse", json!({ "id": "test" }));
    assert!(window.refused("workspace.host.expand", json!({ "id": "nope" })).contains("nope"));
    assert!(window.refused("workspace.host.collapse", json!({ "id": "nope" })).contains("nope"));
}

#[test]
fn file_tabs_follow_the_owned_group_and_feature_tabs_keep_their_data() {
    let mut window = Window::new();
    let console = window.console("console.sql", "select 1;\n");
    let opened = window.run("workspace.tab.open", json!({ "workspace": "test", "path": console, "kind": "console" }));
    assert_eq!((opened["kind"].as_str(), opened["title"].as_str()), (Some("console"), Some("console.sql")));
    assert_eq!(window.active_path(), Some(console.clone()), "a console is a real document");
    let state = window.state();
    assert_eq!(state["expanded"], true);
    assert_eq!(state["active"], opened["id"]);
    let listed = window.run("editor.split.list", json!({}));
    assert_eq!(listed["workspaces"][0]["root"]["tabs"], json!([console]));
    assert_eq!(listed["root"]["tabs"].as_array().unwrap().len(), 2, "the main editor gains no tab");

    let history = window.run(
        "workspace.tab.open",
        json!({ "workspace": "test", "kind": "history", "title": "Query History", "closable": false, "data": { "filter": "x" } }),
    );
    assert_eq!(history["data"]["filter"], "x");
    assert_eq!(window.state()["active"], history["id"]);
    assert_eq!(window.tab_titles(), ["console.sql", "Query History"]);
    assert!(window.refused("workspace.tab.open", json!({ "workspace": "test" })).contains("kind"));
    let again = window.run("workspace.tab.open", json!({ "workspace": "test", "id": history["id"] }));
    assert_eq!(again["data"]["filter"], "x", "the same tab comes to front");
    assert_eq!(window.tab_titles().len(), 2);

    let second = window.console("console_1.sql", "");
    window.run("file.document.open", json!({ "path": second, "group": state["group"] }));
    assert_eq!(window.tab_titles(), ["console.sql", "Query History", "console_1.sql"], "a file opened into the group");
    assert_eq!(window.state()["active"], window.state()["tabs"][2]["id"]);

    let id = window.state()["tabs"][0]["id"].clone();
    window.run("workspace.tab.select", json!({ "workspace": "test", "tab": id }));
    assert_eq!(window.active_path(), Some(console.clone()));
    window.run("workspace.tab.move", json!({ "workspace": "test", "tab": id, "to": 2 }));
    assert_eq!(window.tab_titles(), ["Query History", "console_1.sql", "console.sql"]);
    assert_eq!(window.state()["active"], id, "the moved tab stays active");
    let listed = window.run("editor.split.list", json!({}));
    assert_eq!(listed["workspaces"][0]["root"]["tabs"], json!([second, console]), "the group follows the order");

    let limited = window.run("workspace.host.state", json!({ "id": "test", "limit": 1 }));
    assert_eq!((limited["tabs"].as_array().unwrap().len(), limited["total"].as_u64()), (1, Some(3)));
    assert_eq!(limited["truncated"], true);

    let refused = window.refused("workspace.tab.close", json!({ "workspace": "test", "tab": history["id"] }));
    assert!(refused.contains("Query History"), "{refused}");
    window.run("file.document.close", json!({ "path": console }));
    assert_eq!(window.tab_titles(), ["Query History", "console_1.sql"], "a tab closed elsewhere leaves");
}

#[test]
fn closing_a_file_tab_saves_first_and_an_empty_console_is_deleted() {
    let mut window = Window::new();
    let kept = window.console("console.sql", "select 1;\n");
    let empty = window.console("console_1.sql", "");
    let kept_tab = window.run("workspace.tab.open", json!({ "workspace": "test", "path": kept }))["id"].clone();
    let empty_tab = window.run("workspace.tab.open", json!({ "workspace": "test", "path": empty }))["id"].clone();
    window.run("workspace.tab.select", json!({ "workspace": "test", "tab": kept_tab }));
    window
        .session
        .state_mut::<Workbench>()
        .active_document_mut()
        .unwrap()
        .apply_op(&zen_editor::EditOp::Insert { text: "-- note\n".to_owned() });
    let state = window.run("workspace.tab.close", json!({ "workspace": "test", "tab": kept_tab }));
    assert_eq!(std::fs::read_to_string(&kept).unwrap(), "-- note\nselect 1;\n", "saved before it closed");
    assert_eq!(state["active"], empty_tab, "the tab beside it becomes active");
    assert_eq!(window.active_path(), Some(empty.clone()));
    window.run("workspace.tab.close", json!({ "workspace": "test", "tab": empty_tab }));
    assert!(!empty.exists(), "an empty console is deleted");
    assert!(kept.exists());
    let state = window.state();
    assert_eq!(state["tabs"], json!([]));
    assert_eq!(state["expanded"], true, "the workspace stays expanded and empty");

    let outside = window.project("a.go");
    std::fs::write(&outside, "").unwrap();
    let tab = window.run("workspace.tab.open", json!({ "workspace": "test", "path": outside }))["id"].clone();
    window.run("workspace.tab.close", json!({ "workspace": "test", "tab": tab }));
    assert!(window.project("a.go").exists(), "only consoles are deleted when empty");
}

#[test]
fn close_tab_closes_the_inner_tab_in_front_and_never_hidden_code() {
    let mut window = Window::new();
    let console = window.console("console.sql", "select 1;\n");
    window.run("workspace.tab.open", json!({ "workspace": "test", "path": console }));
    window.run("workspace.tab.open", json!({ "workspace": "test", "kind": "history", "title": "Query History" }));
    let closed = window.run("file.document.close", json!({}));
    assert_eq!(closed["closed"], json!([]), "no file closed: {closed}");
    assert!(closed["tab"].is_string());
    assert_eq!(window.tab_titles(), ["console.sql"]);
    assert_eq!(window.active_path(), Some(console.clone()), "the console beside it is in front");
    window.run("file.document.close", json!({}));
    assert_eq!(window.tab_titles(), Vec::<String>::new(), "then the console closes");
    window.refused("file.document.close", json!({}));
    let listed = window.run("editor.split.list", json!({}));
    assert_eq!(listed["root"]["tabs"].as_array().unwrap().len(), 2, "the code tabs stay");

    window.run(
        "workspace.tab.open",
        json!({ "workspace": "test", "kind": "sessions", "title": "Sessions", "closable": false }),
    );
    assert!(window.refused("file.document.close", json!({})).contains("Sessions"), "a tab that does not close says so");
    window.run("workspace.host.collapse", json!({}));
    window.run("file.document.close", json!({}));
    let listed = window.run("editor.split.list", json!({}));
    assert_eq!(listed["root"]["tabs"].as_array().unwrap().len(), 1, "collapsed, the code tab closes");
    assert_eq!(window.tab_titles(), ["Sessions"]);
}

#[test]
fn editor_commands_are_refused_on_a_tab_that_is_not_a_file() {
    let mut window = Window::new();
    let console = window.console("console.sql", "select 1;\n");
    window.run("workspace.tab.open", json!({ "workspace": "test", "path": console }));
    assert_eq!(super::refusal(&window.session, "editor.edit.insert", &json!({})), None, "a console is a text file");
    window.run("workspace.tab.open", json!({ "workspace": "test", "kind": "history", "title": "Query History" }));
    let reason = super::refusal(&window.session, "editor.edit.insert", &json!({})).unwrap();
    assert!(reason.contains("Query History") && reason.contains("Test") && reason.contains("editor.edit.insert"));
    assert_eq!(super::refusal(&window.session, "editor.split.list", &json!({})), None);
    assert_eq!(super::refusal(&window.session, "editor.fold.list", &json!({ "path": console })), None);
    assert_eq!(super::refusal(&window.session, "file.document.save", &json!({})), None);
    window.run("workspace.host.collapse", json!({}));
    assert_eq!(
        super::refusal(&window.session, "editor.edit.insert", &json!({})),
        None,
        "collapsed, the code is in front"
    );
}

#[test]
fn saved_workspaces_come_back_in_order_with_carets_and_wait_for_their_feature() {
    let mut window = Window::new();
    let console = window.console("console.sql", "select 1;\nselect 2;\n");
    let gone = window.console("gone.sql", "select 0;\n");
    window.run("workspace.tab.open", json!({ "workspace": "test", "path": gone }));
    window.run(
        "workspace.tab.open",
        json!({ "workspace": "test", "kind": "history", "title": "Query History", "data": { "n": 1 } }),
    );
    window.run("workspace.tab.open", json!({ "workspace": "test", "path": console }));
    window.run("editor.caret.goTo", json!({ "line": 2, "column": 4 }));
    let history = window.state()["tabs"][1]["id"].clone();
    window.run("workspace.tab.select", json!({ "workspace": "test", "tab": history }));
    window.run("workspace.host.collapse", json!({}));
    let saved = super::saved(&mut window.session);
    assert_eq!(saved.len(), 1);
    assert!(!saved[0].expanded);
    assert_eq!(saved[0].active.as_deref(), history.as_str());
    let caret = saved[0].tabs[2].caret;
    assert_eq!(caret, Some(13), "line 2, column 4");
    std::fs::remove_file(&gone).unwrap();

    let mut restarted = Window::new();
    restarted.session.state_mut::<Workspaces>().unregister("test");
    super::restore(&mut restarted.session, saved.clone());
    assert_eq!(super::saved(&mut restarted.session), saved, "kept as it was while the feature is not registered");
    let info = WorkspaceInfo { id: "test".into(), title: "Test".into(), icon: "database".into() };
    restarted.session.state_mut::<Workspaces>().register(info);
    let state = restarted.state();
    assert_eq!(restarted.tab_titles(), ["Query History", "console.sql"], "a missing file drops its tab");
    assert_eq!(state["active"], history, "the tab that was active");
    assert_eq!(state["expanded"], false, "collapsed as it was left");
    assert_eq!(restarted.active_path(), Some(restarted.project("b.go")), "the code keeps the keyboard");
    restarted.run("workspace.host.expand", json!({ "id": "test" }));
    assert_eq!(restarted.state()["active"], history, "expanding keeps the tab that is not a file in front");
    let console_tab = restarted.state()["tabs"][1]["id"].clone();
    restarted.run("workspace.tab.select", json!({ "workspace": "test", "tab": console_tab }));
    let head = restarted.session.state::<Workbench>().unwrap().active_document().unwrap().selections().primary().head;
    assert_eq!(head, 13, "the console's caret comes back");
    let opened = restarted.run("workspace.tab.open", json!({ "workspace": "test", "kind": "x", "title": "New" }));
    assert_ne!(opened["id"], history, "new tabs never reuse a kept id");
}

#[test]
fn only_consoles_are_saved_on_idle() {
    let mut window = Window::new();
    let console = window.console("console.sql", "");
    window.run("workspace.tab.open", json!({ "workspace": "test", "path": console }));
    window.run("editor.edit.insert", json!({ "text": "select 1;" }));
    window.run("workspace.host.collapse", json!({}));
    window.run("editor.edit.insert", json!({ "text": "// code" }));
    assert_eq!(super::dirty_console_versions(&window.session).len(), 1);
    let (saved, failed) = super::save_consoles(&mut window.session);
    assert_eq!((saved, failed.len()), (vec![console.clone()], 0));
    assert_eq!(std::fs::read_to_string(&console).unwrap(), "select 1;");
    assert_eq!(std::fs::read_to_string(window.project("b.go")).unwrap(), "package b\n", "code waits for autosave");
}

#[test]
fn switching_project_keeps_the_workspace_tabs_with_their_project() {
    let mut window = Window::new();
    let first = window.dir.path().join("project");
    let console = window.console("console.sql", "");
    window.run("workspace.tab.open", json!({ "workspace": "test", "path": console }));
    window.run("editor.edit.insert", json!({ "text": "select 1;" }));
    window.run("workspace.tab.open", json!({ "workspace": "test", "kind": "history", "title": "Query History" }));
    let other = window.dir.path().join("other");
    std::fs::create_dir(&other).unwrap();
    let mut remembered = super::SavedWorkspace { id: "test".into(), expanded: true, ..Default::default() };
    remembered.tabs.push(super::SavedTab {
        id: "t9".into(),
        kind: "sessions".into(),
        title: "Sessions".into(),
        path: None,
        closable: true,
        data: Value::Null,
        caret: None,
        first_line: None,
        group: None,
    });
    window.session.state_mut::<Workspaces>().remember_project(other.clone(), vec![remembered]);

    window.run("file.project.open", json!({ "path": other }));
    assert_eq!(std::fs::read_to_string(&console).unwrap(), "select 1;", "the consoles are saved first");
    let left = window.session.state_mut::<Workspaces>().take_left();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].0, first);
    assert_eq!(left[0].1[0].tabs.len(), 2, "the old project's tabs go to its workspace.json");
    assert_eq!(window.tab_titles(), ["Sessions"], "the new project's tabs come back");
    assert_eq!(window.state()["expanded"], false, "collapsed");

    window.run("file.project.open", json!({ "path": first }));
    assert_eq!(window.tab_titles(), ["console.sql", "Query History"], "and back again");
    assert_eq!(window.state()["expanded"], false);
}

#[test]
fn an_open_workspace_stays_open_when_collapsed_and_closing_keeps_its_tabs() {
    fn opened(session: &mut Session) -> Result<(), zen_core::CommandError> {
        session.state_mut::<Vec<&'static str>>().push("opened");
        Ok(())
    }
    fn closed(session: &mut Session) {
        session.state_mut::<Vec<&'static str>>().push("closed");
    }
    let mut window = Window::new();
    let workspaces = window.session.state_mut::<Workspaces>();
    workspaces.on_open("test", opened);
    workspaces.on_close("test", closed);
    assert_eq!(window.state()["open"], false, "not open until it expands");

    window.run("workspace.host.expand", json!({ "id": "test" }));
    window.run("workspace.tab.open", json!({ "workspace": "test", "kind": "history", "title": "Query History" }));
    window.run("workspace.host.expand", json!({ "id": "test" }));
    window.run("workspace.host.collapse", json!({}));
    let state = window.state();
    assert_eq!((state["open"].clone(), state["expanded"].clone()), (json!(true), json!(false)), "{state}");
    let saved = super::saved(&mut window.session);
    assert!(saved[0].open && !saved[0].expanded, "kept open across restarts: {saved:?}");

    let list = window.run("workspace.host.close", json!({ "id": "test" }));
    assert_eq!(list["workspaces"][0]["open"], false, "{list}");
    let state = window.state();
    assert_eq!(state["tabs"][0]["title"], "Query History", "closing keeps the inner tabs: {state}");
    assert_eq!(*window.session.state::<Vec<&'static str>>().unwrap(), ["opened", "closed"], "each hook ran once");

    window.run("workspace.host.expand", json!({ "id": "test" }));
    window.run("workspace.host.close", json!({ "id": "test" }));
    let state = window.state();
    assert_eq!((state["open"].clone(), state["expanded"].clone()), (json!(false), json!(false)), "× collapses it too");
    assert!(window.refused("workspace.host.close", json!({ "id": "nothing" })).contains("nothing"));

    let mut restarted = Window::new();
    let mut kept = saved;
    kept[0].tabs.clear();
    super::restore(&mut restarted.session, kept);
    let state = restarted.state();
    assert_eq!((state["open"].clone(), state["expanded"].clone()), (json!(true), json!(false)), "{state}");
}

fn panes(state: &Value) -> Vec<(u64, Value)> {
    state["panes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|pane| (pane["group"].as_u64().unwrap(), pane["active"].clone()))
        .collect()
}

fn tab_of<'a>(state: &'a Value, id: &Value) -> &'a Value {
    state["tabs"].as_array().unwrap().iter().find(|tab| tab["id"] == *id).unwrap()
}

#[test]
fn a_workspace_area_splits_and_its_tabs_move_between_its_groups() {
    let mut window = Window::new();
    let console = window.console("console.sql", "select 1;\n");
    let file = window.run("workspace.tab.open", json!({ "workspace": "test", "path": console }))["id"].clone();
    let history = window
        .run("workspace.tab.open", json!({ "workspace": "test", "kind": "history", "title": "Query History" }))["id"]
        .clone();
    let first = window.state()["group"].as_u64().unwrap();
    assert_eq!(panes(&window.state()), [(first, history.clone())]);

    let state = window.run("workspace.tab.split", json!({ "workspace": "test", "tab": history, "direction": "right" }));
    let Ok([(left, left_shown), (right, right_shown)]) = <[_; 2]>::try_from(panes(&state)) else { panic!("{state}") };
    assert_eq!((left, &left_shown, &right_shown), (first, &file, &history), "a tab the feature draws moves");
    assert_eq!((state["group"].as_u64(), state["expanded"].as_bool()), (Some(right), Some(true)));
    assert_eq!(state["active"], history);
    assert_eq!(tab_of(&state, &history)["group"], right);
    assert_eq!(window.focused(), right);
    let listed = window.run("editor.split.list", json!({}));
    assert_eq!(listed["workspaces"][0]["root"]["direction"], "right");
    assert_eq!(listed["root"]["tabs"].as_array().unwrap().len(), 2, "the main editor is not split");

    let alone =
        window.refused("workspace.tab.split", json!({ "workspace": "test", "tab": history, "direction": "down" }));
    assert!(alone.contains("Query History"), "{alone}");

    window.run("editor.split.focus", json!({ "group": left }));
    assert_eq!(window.active_path(), Some(console.clone()));
    let state = window.run("editor.split.down", json!({}));
    assert_eq!(state["workspaces"][0]["root"]["first"]["direction"], "down", "Split Down splits the workspace's group");
    let state = window.state();
    assert_eq!(panes(&state).len(), 3);
    let copies: Vec<&Value> =
        state["tabs"].as_array().unwrap().iter().filter(|tab| tab["path"] == json!(console)).collect();
    assert_eq!(copies.len(), 2, "the console shows in both groups");
    let below = window.focused();
    assert_ne!(below, left);
    assert_eq!(window.active_path(), Some(console.clone()));

    window.run("editor.split.next", json!({}));
    assert_eq!(window.focused(), right, "the next splitter goes round the workspace's groups");
    assert_eq!(window.state()["expanded"], true);

    let state = window.run("workspace.tab.move", json!({ "workspace": "test", "tab": history, "group": left }));
    assert_eq!(panes(&state).len(), 2, "a group left without tabs goes");
    assert_eq!(tab_of(&state, &history)["group"], left);
    assert_eq!((state["group"].as_u64(), state["active"].clone()), (Some(left), history.clone()));

    let copies: Vec<Value> = state["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|tab| tab["path"] == json!(console))
        .map(|tab| tab["id"].clone())
        .collect();
    for copy in copies {
        window.run("workspace.tab.close", json!({ "workspace": "test", "tab": copy }));
    }
    let state = window.state();
    assert_eq!(panes(&state).len(), 1, "closing the file in every group leaves one group");
    assert_eq!(window.tab_titles(), ["Query History"]);
    assert!(
        window.registry.dispatch(&mut window.session, "editor.split.unsplit", json!({})).is_err(),
        "nothing to unsplit"
    );
}

#[test]
fn unsplit_in_a_workspace_gathers_its_tabs_into_the_focused_group() {
    let mut window = Window::new();
    let a = window.console("a.sql", "select 1;\n");
    let b = window.console("b.sql", "select 2;\n");
    window.run("workspace.tab.open", json!({ "workspace": "test", "path": a }));
    let b_tab = window.run("workspace.tab.open", json!({ "workspace": "test", "path": b }))["id"].clone();
    let history = window
        .run("workspace.tab.open", json!({ "workspace": "test", "kind": "history", "title": "Query History" }))["id"]
        .clone();
    window.run("workspace.tab.split", json!({ "workspace": "test", "tab": history, "direction": "down" }));
    window.run("workspace.tab.split", json!({ "workspace": "test", "tab": b_tab, "direction": "right", "move": true }));
    assert_eq!(panes(&window.state()).len(), 3);
    window.run("editor.split.unsplitAll", json!({}));
    let state = window.state();
    let [(only, _)] = panes(&state)[..] else { panic!("{state}") };
    assert!(state["tabs"].as_array().unwrap().iter().all(|tab| tab["group"] == only), "{state}");
    assert_eq!(window.tab_titles().len(), 3);
    assert_eq!(window.state()["expanded"], true);
}

fn takes_sql(_: &Session, path: &std::path::Path) -> bool {
    path.extension().is_some_and(|extension| extension == "sql") && !path.to_string_lossy().contains("consoles")
}

#[test]
fn project_scripts_move_between_the_workspace_and_the_editor_and_consoles_stay() {
    let mut window = Window::new();
    window.session.state_mut::<Workspaces>().accept_files("test", takes_sql);
    let script = window.project("report.sql");
    std::fs::write(&script, "select 1;\n").unwrap();
    window.run("file.document.open", json!({ "path": script }));
    let main = window.run("editor.split.list", json!({}));
    assert_eq!(main["root"]["tabs"].as_array().unwrap().len(), 3);

    let groups = window.run("editor.split.moveTab", json!({ "from": 1, "index": 2, "workspace": "test" }));
    assert_eq!(groups["root"]["tabs"].as_array().unwrap().len(), 2, "the script leaves the code editor");
    let state = window.state();
    assert_eq!(state["expanded"], true, "and the workspace expands on it");
    assert_eq!(window.tab_titles(), ["report.sql"]);
    assert_eq!(window.active_path(), Some(script.clone()));
    let tab = state["active"].clone();

    let state = window.run("workspace.tab.move", json!({ "workspace": "test", "tab": tab, "group": 1 }));
    assert_eq!(state["expanded"], false, "back in the code editor, which takes the keyboard");
    assert_eq!(state["tabs"], json!([]));
    let main = window.run("editor.split.list", json!({}));
    assert_eq!(main["root"]["tabs"][2], json!(script));

    let code = window.registry.dispatch(
        &mut window.session,
        "editor.split.moveTab",
        json!({ "from": 1, "index": 0, "workspace": "test" }),
    );
    let zen_core::DispatchError::Failed { error, .. } = code.unwrap_err() else { panic!("not refused") };
    assert_eq!(error.code.as_deref(), Some("notMovable"));
    assert!(error.message.contains("a.go"), "{}", error.message);

    let console = window.console("console.sql", "select 2;\n");
    let console_tab = window.run("workspace.tab.open", json!({ "workspace": "test", "path": console }))["id"].clone();
    let refused = window.refused("workspace.tab.move", json!({ "workspace": "test", "tab": console_tab, "group": 1 }));
    assert!(refused.contains("console.sql"), "{refused}");
    let history = window
        .run("workspace.tab.open", json!({ "workspace": "test", "kind": "history", "title": "Query History" }))["id"]
        .clone();
    let refused = window.refused("workspace.tab.move", json!({ "workspace": "test", "tab": history, "group": 1 }));
    assert!(refused.contains("Query History"), "{refused}");
    assert_eq!(window.tab_titles(), ["console.sql", "Query History"]);
}

#[test]
fn a_split_workspace_comes_back_after_a_restart() {
    let mut window = Window::new();
    let a = window.console("a.sql", "select 1;\n");
    let b = window.console("b.sql", "select 2;\n");
    window.run("workspace.tab.open", json!({ "workspace": "test", "path": a }));
    let b_tab = window.run("workspace.tab.open", json!({ "workspace": "test", "path": b }))["id"].clone();
    let history = window
        .run("workspace.tab.open", json!({ "workspace": "test", "kind": "history", "title": "Query History" }))["id"]
        .clone();
    window.run("workspace.tab.split", json!({ "workspace": "test", "tab": b_tab, "direction": "right", "move": true }));
    let before = window.state();
    let right = before["group"].clone();
    let _ = window.run("editor.split.resize", json!({ "before": right, "ratio": 0.3 }));
    let saved = super::saved(&mut window.session);
    let layout = saved[0].layout.clone().expect("the split is kept");
    let crate::groups::SplitNode::Split { ratio, .. } = layout else { panic!("a split") };
    assert!((ratio - 0.3).abs() < 1e-6);
    assert_eq!(saved[0].shown.len(), 2);
    let kept = serde_json::to_value(&saved).unwrap();
    assert!(kept[0]["layout"]["first"]["tabs"].as_array().is_none_or(Vec::is_empty), "no files in the layout: {kept}");

    let mut restarted = Window::new();
    super::restore(&mut restarted.session, saved);
    let state = restarted.state();
    let Ok([(left, left_shown), (right, right_shown)]) = <[_; 2]>::try_from(panes(&state)) else { panic!("{state}") };
    assert_eq!(left_shown, history, "each group shows what it showed");
    let b_back = state["tabs"].as_array().unwrap().iter().find(|tab| tab["path"] == json!(b)).unwrap();
    assert_eq!(right_shown, b_back["id"]);
    assert_eq!(b_back["group"], right);
    assert_eq!(tab_of(&state, &history)["group"], left);
    assert_eq!(state["expanded"], true);
    assert_eq!(restarted.focused(), right, "the group that had the keyboard");
    assert_eq!(restarted.active_path(), Some(b.clone()));
}
