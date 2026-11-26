use std::path::{Path, PathBuf};

use serde_json::json;
use zen_core::{Registry, Session};
use zen_editor::{EditOp, Selection, Selections};

use super::{EditorGroups, OWNED_FIRST, SplitDirection, SplitNode, WINDOW_FIRST, WindowBounds};
use crate::Workbench;

fn path(name: &str) -> PathBuf {
    PathBuf::from(format!("/p/{name}"))
}

fn opened(names: &[&str]) -> EditorGroups {
    let mut groups = EditorGroups::default();
    for name in names {
        groups.open(&path(name));
    }
    groups
}

fn tabs(groups: &EditorGroups) -> Vec<Vec<String>> {
    groups
        .groups()
        .iter()
        .map(|group| group.tabs.iter().map(|tab| tab.file_name().unwrap().to_string_lossy().into_owned()).collect())
        .collect()
}

fn active(groups: &EditorGroups) -> Option<String> {
    groups.focused_group().active_path().map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
}

#[test]
fn a_split_shows_the_active_tab_in_a_new_focused_group() {
    let mut groups = opened(&["a.go", "b.go"]);
    assert!(!groups.is_split());
    let new = groups.split(1, None, SplitDirection::Right, false).unwrap();
    assert_eq!(new, 2);
    assert!(groups.is_split());
    assert_eq!(tabs(&groups), [vec!["a.go", "b.go"], vec!["b.go"]]);
    assert_eq!(groups.focused(), 2);
    assert_eq!(active(&groups).as_deref(), Some("b.go"));
    let SplitNode::Split { direction, ratio, .. } = groups.root() else { panic!("split") };
    assert_eq!((*direction, *ratio), (SplitDirection::Right, 0.5));

    let down = groups.split(2, None, SplitDirection::Down, false).unwrap();
    assert_eq!(tabs(&groups), [vec!["a.go", "b.go"], vec!["b.go"], vec!["b.go"]]);
    let SplitNode::Split { second, .. } = groups.root() else { panic!("split") };
    assert!(matches!(second.as_ref(), SplitNode::Split { direction: SplitDirection::Down, .. }));
    assert_eq!(groups.focused(), down);
}

#[test]
fn split_and_move_takes_the_tab_out_but_never_empties_the_group() {
    let mut groups = opened(&["a.go", "b.go"]);
    groups.split(1, Some(0), SplitDirection::Right, true).unwrap();
    assert_eq!(tabs(&groups), [vec!["b.go"], vec!["a.go"]]);
    assert_eq!(groups.split(1, None, SplitDirection::Down, true), None, "the only tab cannot move");
    assert_eq!(groups.split(1, Some(7), SplitDirection::Down, false), None, "no such tab");
    assert_eq!(groups.split(9, None, SplitDirection::Down, false), None, "no such group");
}

#[test]
fn closing_the_last_tab_of_a_split_removes_it() {
    let mut groups = opened(&["a.go", "b.go"]);
    groups.split(1, None, SplitDirection::Right, false);
    assert_eq!(groups.close(2, 0), Some(path("b.go")));
    assert!(!groups.is_split());
    assert_eq!(groups.focused(), 1);
    assert!(groups.contains(&path("b.go")), "the first group still shows it");
    assert_eq!(groups.close(1, 0), Some(path("a.go")));
    assert_eq!(groups.close(1, 0), Some(path("b.go")));
    assert_eq!(tabs(&groups), [Vec::<String>::new()], "the only group stays, empty");
    assert_eq!(groups.close(1, 0), None);
}

#[test]
fn closing_the_active_tab_activates_the_one_on_its_left() {
    let mut groups = opened(&["a.go", "b.go", "c.go"]);
    groups.select(1, 1);
    groups.close(1, 1);
    assert_eq!(active(&groups).as_deref(), Some("a.go"));
    groups.close(1, 0);
    assert_eq!(active(&groups).as_deref(), Some("c.go"), "the first tab hands over to the right");
}

#[test]
fn the_focus_goes_round_the_groups_in_order() {
    let mut groups = opened(&["a.go"]);
    groups.split(1, None, SplitDirection::Right, false);
    groups.split(2, None, SplitDirection::Down, false);
    groups.focus(1);
    let mut seen = Vec::new();
    for _ in 0..4 {
        groups.focus_next(true);
        seen.push(groups.focused());
    }
    assert_eq!(seen, [2, 3, 1, 2]);
    groups.focus_next(false);
    groups.focus_next(false);
    assert_eq!(groups.focused(), 3);
    assert!(!groups.focus(9));
    assert!(!EditorGroups::default().focus_next(true), "nothing to go to without a split");
}

#[test]
fn unsplit_keeps_the_focused_group_with_the_other_tabs_after_its_own() {
    let mut groups = opened(&["a.go", "b.go"]);
    groups.split(1, None, SplitDirection::Right, true);
    groups.open(&path("c.go"));
    assert_eq!(tabs(&groups), [vec!["a.go"], vec!["b.go", "c.go"]]);
    groups.focus(1);
    assert!(groups.unsplit());
    assert_eq!(tabs(&groups), [vec!["a.go", "b.go", "c.go"]]);
    assert_eq!(active(&groups).as_deref(), Some("a.go"));
    assert!(!groups.unsplit(), "nothing left to unsplit");
}

#[test]
fn unsplit_removes_only_the_split_the_group_is_in() {
    let mut groups = opened(&["a.go"]);
    groups.split(1, None, SplitDirection::Right, false);
    groups.open(&path("b.go"));
    groups.split(2, None, SplitDirection::Down, false);
    groups.open(&path("c.go"));
    assert_eq!(tabs(&groups), [vec!["a.go"], vec!["a.go", "b.go"], vec!["b.go", "c.go"]]);
    assert!(groups.unsplit());
    assert_eq!(tabs(&groups), [vec!["a.go"], vec!["b.go", "c.go", "a.go"]]);
    assert_eq!(groups.focused(), 3);
    assert!(groups.unsplit_all());
    assert_eq!(tabs(&groups), [vec!["b.go", "c.go", "a.go"]]);
    assert!(!groups.unsplit_all());
}

#[test]
fn opening_a_file_brings_the_group_that_shows_it_to_the_front() {
    let mut groups = opened(&["a.go", "b.go"]);
    groups.split(1, Some(0), SplitDirection::Right, true);
    assert_eq!(groups.focused(), 2);
    groups.open(&path("b.go"));
    assert_eq!(groups.focused(), 1, "b.go is only in the first group");
    groups.open(&path("c.go"));
    assert_eq!(tabs(&groups), [vec!["b.go", "c.go"], vec!["a.go"]], "a new file opens in the focused group");
}

#[test]
fn a_tab_moves_to_another_group() {
    let mut groups = opened(&["a.go", "b.go"]);
    groups.split(1, None, SplitDirection::Right, false);
    groups.focus(1);
    assert!(groups.move_tab(1, 0, 2, Some(0)));
    assert_eq!(tabs(&groups), [vec!["b.go"], vec!["a.go", "b.go"]]);
    assert_eq!((groups.focused(), active(&groups).as_deref()), (2, Some("a.go")));
    assert!(groups.move_tab(1, 0, 2, None), "a file the target shows only leaves the source");
    assert!(!groups.is_split(), "the emptied group goes");
    assert_eq!(tabs(&groups), [vec!["a.go", "b.go"]]);
    assert!(!groups.move_tab(2, 0, 2, None));
    assert!(!groups.move_tab(2, 0, 5, None));
}

#[test]
fn a_divider_moves_within_range() {
    let mut groups = opened(&["a.go"]);
    groups.split(1, None, SplitDirection::Right, false);
    groups.split(2, None, SplitDirection::Down, false);
    assert!(groups.resize(2, 0.3));
    assert!(groups.resize(3, 2.0));
    assert!(!groups.resize(1, 0.5), "no divider before the first group");
    assert!(!groups.resize(2, f32::NAN));
    let SplitNode::Split { ratio, second, .. } = groups.root() else { panic!("split") };
    assert_eq!(*ratio, 0.3);
    let SplitNode::Split { ratio, .. } = second.as_ref() else { panic!("split") };
    assert_eq!(*ratio, 0.9);
}

#[test]
fn restored_groups_keep_the_open_files_and_renumber() {
    let mut groups = opened(&["a.go", "gone.go"]);
    groups.split(1, Some(1), SplitDirection::Right, true);
    groups.open(&path("b.go"));
    groups.split(2, Some(0), SplitDirection::Down, true);
    assert_eq!(tabs(&groups), [vec!["a.go"], vec!["b.go"], vec!["gone.go"]]);
    groups.focus(2);
    let json = serde_json::to_value(&groups).unwrap();
    assert_eq!(json["focused"], 2);
    assert_eq!(json["root"]["kind"], "split");
    assert_eq!(json["root"]["direction"], "right");
    assert_eq!(json["root"]["first"]["kind"], "group");
    assert_eq!(json["root"]["first"]["tabs"][0], "/p/a.go");
    let read: EditorGroups = serde_json::from_value(json).unwrap();
    assert_eq!(read, groups);

    let restored = read.restored(|path| path != Path::new("/p/gone.go"));
    assert_eq!(tabs(&restored), [vec!["a.go"], vec!["b.go"]]);
    assert_eq!(restored.groups().iter().map(|group| group.id).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(restored.focused(), 2);
    assert_eq!(active(&restored).as_deref(), Some("b.go"));

    let broken: EditorGroups = serde_json::from_value(json!({
        "focused": 7,
        "root": { "kind": "split", "direction": "down", "ratio": 5.0,
                  "first": { "kind": "group", "id": 3, "tabs": ["/p/a.go"], "active": 4 },
                  "second": { "kind": "group", "id": 3, "tabs": ["/p/b.go"] } }
    }))
    .unwrap();
    let restored = broken.restored(|_| true);
    assert_eq!(
        restored.groups().iter().map(|group| (group.id, group.active)).collect::<Vec<_>>(),
        [(1, Some(0)), (2, Some(0))]
    );
    assert_eq!(restored.focused(), 1);
    let SplitNode::Split { ratio, .. } = restored.root() else { panic!("split") };
    assert_eq!(*ratio, 0.9);
    assert!(!EditorGroups::default().restored(|_| false).is_split());
}

fn session() -> (Registry, Session, tempfile::TempDir) {
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.go"), "package a\n\nfunc A() {}\n").unwrap();
    std::fs::write(dir.path().join("b.go"), "package b\n").unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
    for name in ["a.go", "b.go"] {
        registry.dispatch(&mut session, "file.document.open", json!({ "path": dir.path().join(name) })).unwrap();
    }
    (registry, session, dir)
}

fn workbench(session: &mut Session) -> &mut Workbench {
    session.state_mut::<Workbench>()
}

fn caret(session: &mut Session) -> usize {
    workbench(session).active_document().unwrap().selections().primary().head
}

#[test]
fn one_document_is_shown_by_both_groups_with_carets_of_their_own() {
    let (registry, mut session, dir) = session();
    let a = dir.path().join("a.go");
    registry.dispatch(&mut session, "editor.tab.select", json!({ "index": 0 })).unwrap();
    workbench(&mut session).active_document_mut().unwrap().set_selections(Selections::single(Selection::caret(8)));
    let split = registry.dispatch(&mut session, "editor.split.right", json!({})).unwrap();
    assert_eq!(split["focused"], 2);
    assert_eq!(split["root"]["second"]["tabs"], json!([a]));
    assert_eq!(workbench(&mut session).documents().len(), 2, "one document per file");
    assert_eq!(caret(&mut session), 8, "the new group starts where the old one was");

    workbench(&mut session).active_document_mut().unwrap().set_selections(Selections::single(Selection::caret(0)));
    workbench(&mut session).active_document_mut().unwrap().apply_op(&EditOp::Insert { text: "// x\n".to_owned() });
    assert_eq!(caret(&mut session), 5);

    registry.dispatch(&mut session, "editor.split.previous", json!({})).unwrap();
    let document = workbench(&mut session).active_document().unwrap();
    assert!(document.text().starts_with("// x\npackage a"), "the edit shows in the other group");
    assert_eq!(document.selections().primary().head, 13, "its caret moved with the text before it");

    let next = registry.dispatch(&mut session, "editor.split.next", json!({})).unwrap();
    assert_eq!(next["focused"], 2);
    assert_eq!(caret(&mut session), 5);
    workbench(&mut session).present(1);
    assert_eq!(caret(&mut session), 13, "presenting a group shows its carets without focusing it");
    assert_eq!(workbench(&mut session).groups().focused(), 2);
    workbench(&mut session).present(2);
    assert_eq!(caret(&mut session), 5);
}

#[test]
fn closing_tabs_closes_the_document_only_when_no_group_shows_it() {
    let (registry, mut session, dir) = session();
    registry.dispatch(&mut session, "editor.split.down", json!({})).unwrap();
    registry.dispatch(&mut session, "file.document.close", json!({ "group": 1, "index": 1 })).unwrap();
    assert_eq!(workbench(&mut session).documents().len(), 2, "the second group still shows b.go");
    assert_eq!(workbench(&mut session).groups().focused(), 2, "closing elsewhere leaves the focus");
    registry.dispatch(&mut session, "file.document.close", json!({})).unwrap();
    assert!(!workbench(&mut session).groups().is_split(), "the emptied group goes");
    let paths: Vec<PathBuf> =
        workbench(&mut session).documents().iter().map(|document| document.path().to_owned()).collect();
    assert_eq!(paths, [dir.path().join("a.go")]);
    assert_eq!(workbench(&mut session).active_document().unwrap().path(), dir.path().join("a.go"));
}

#[test]
fn tabs_of_another_group_are_selected_and_moved_by_commands() {
    let (registry, mut session, dir) = session();
    registry.dispatch(&mut session, "editor.split.right", json!({ "index": 0, "move": true })).unwrap();
    let listed = registry.dispatch(&mut session, "editor.split.list", json!({})).unwrap();
    assert_eq!(listed["root"]["first"]["tabs"], json!([dir.path().join("b.go")]));
    assert_eq!(listed["root"]["second"]["tabs"], json!([dir.path().join("a.go")]));
    registry.dispatch(&mut session, "editor.tab.select", json!({ "group": 1, "index": 0 })).unwrap();
    assert_eq!(workbench(&mut session).groups().focused(), 1);
    assert_eq!(workbench(&mut session).active_document().unwrap().path(), dir.path().join("b.go"));
    registry.dispatch(&mut session, "editor.split.focus", json!({ "group": 2 })).unwrap();
    assert_eq!(workbench(&mut session).active_document().unwrap().path(), dir.path().join("a.go"));
    registry.dispatch(&mut session, "editor.split.resize", json!({ "before": 2, "ratio": 0.25 })).unwrap();
    let moved =
        registry.dispatch(&mut session, "editor.split.moveTab", json!({ "from": 1, "index": 0, "to": 2 })).unwrap();
    assert_eq!(moved["root"]["kind"], "group");
    assert_eq!(moved["root"]["tabs"], json!([dir.path().join("a.go"), dir.path().join("b.go")]));
    assert!(registry.dispatch(&mut session, "editor.split.unsplit", json!({})).is_err(), "not split");
    assert!(!registry.is_available(&session, "editor.split.unsplitAll"));
    assert!(registry.dispatch(&mut session, "editor.split.focus", json!({ "group": 9 })).is_err());
}

#[test]
fn files_that_are_gone_leave_every_group() {
    let mut groups = opened(&["a.go", "b.go"]);
    groups.split(1, None, SplitDirection::Right, false);
    groups.open(&path("c.go"));
    groups.split(2, Some(0), SplitDirection::Down, true);
    assert_eq!(tabs(&groups), [vec!["a.go", "b.go"], vec!["c.go"], vec!["b.go"]]);
    groups.retain(|path| path != Path::new("/p/b.go"));
    assert_eq!(tabs(&groups), [vec!["a.go"], vec!["c.go"]]);
    assert_eq!(groups.focused(), 2, "the focus goes to the group before the one that went");
}

#[test]
fn undo_puts_the_caret_of_the_focused_group_and_moves_the_other_along() {
    let (registry, mut session, _dir) = session();
    registry.dispatch(&mut session, "editor.tab.select", json!({ "index": 0 })).unwrap();
    workbench(&mut session).active_document_mut().unwrap().set_selections(Selections::single(Selection::caret(20)));
    registry.dispatch(&mut session, "editor.split.right", json!({})).unwrap();
    workbench(&mut session).present(1);
    workbench(&mut session).present(2);
    workbench(&mut session).active_document_mut().unwrap().set_selections(Selections::single(Selection::caret(0)));
    workbench(&mut session).active_document_mut().unwrap().apply_op(&EditOp::Insert { text: "// x\n".to_owned() });
    workbench(&mut session).present(1);
    assert_eq!(caret(&mut session), 25, "the other group's caret moved with the edit");
    workbench(&mut session).present(2);

    registry.dispatch(&mut session, "editor.edit.undo", json!({})).unwrap();
    assert_eq!(workbench(&mut session).groups().focused(), 2);
    assert_eq!(caret(&mut session), 0, "the focused group's caret goes to the undone change");
    registry.dispatch(&mut session, "editor.split.focus", json!({ "group": 1 })).unwrap();
    assert_eq!(caret(&mut session), 20, "the other group's caret moved back with the text");
}

#[test]
fn open_in_right_split_moves_a_file_that_was_not_open_and_copies_one_that_was() {
    let (registry, mut session, dir) = session();
    let c = dir.path().join("c.go");
    std::fs::write(&c, "package c\n").unwrap();
    let split = registry.dispatch(&mut session, "editor.split.openRight", json!({ "path": c })).unwrap();
    assert_eq!(split["focused"], 2);
    assert_eq!(split["root"]["first"]["tabs"], json!([dir.path().join("a.go"), dir.path().join("b.go")]));
    assert_eq!(split["root"]["second"]["tabs"], json!([c]), "not left in the first group");
    assert_eq!(workbench(&mut session).active_document().unwrap().path(), c);
    registry.dispatch(&mut session, "editor.split.unsplitAll", json!({})).unwrap();
    let a = dir.path().join("a.go");
    let split = registry.dispatch(&mut session, "editor.split.openRight", json!({ "path": a })).unwrap();
    assert!(split["root"]["first"]["tabs"].as_array().unwrap().contains(&json!(a)), "an open tab stays: {split}");
    assert_eq!(split["root"]["second"]["tabs"], json!([a]));
    let missing = json!({ "path": dir.path().join("x.go") });
    assert!(registry.dispatch(&mut session, "editor.split.openRight", missing).is_err());
}

fn owned_tabs(groups: &EditorGroups, workspace: &str) -> Vec<String> {
    let group = groups.owned(workspace).unwrap();
    group.tabs.iter().map(|tab| tab.file_name().unwrap().to_string_lossy().into_owned()).collect()
}

#[test]
fn an_owned_group_has_a_stable_id_outside_the_split_tree_and_stays_when_empty() {
    let mut groups = opened(&["a.go"]);
    let db = groups.own("database");
    assert_eq!(db, OWNED_FIRST);
    assert_eq!(groups.own("database"), db, "one group per workspace");
    assert_eq!(groups.own("git"), OWNED_FIRST + 1);
    assert_eq!(groups.focused(), 1, "owning a group does not focus it");
    assert!(!groups.is_split(), "an owned group is no split");
    assert_eq!(groups.groups().len(), 1, "nor one of the main groups");
    assert_eq!(groups.owner(db), Some("database"));
    assert_eq!(groups.owner(1), None);

    assert!(groups.open_in(&path("console.sql"), db));
    assert_eq!(groups.expanded(), Some("database"), "its group has the keyboard");
    assert_eq!(groups.main_focused(), 1, "the main group to return to");
    assert_eq!(groups.close(db, 0), Some(path("console.sql")));
    assert_eq!(groups.owned("database").map(|group| group.tabs.len()), Some(0), "an empty owned group stays");
    assert_eq!(groups.focused(), db);
    assert!(!groups.open_in(&path("x.sql"), 77), "no such group");

    groups.split(1, None, SplitDirection::Right, false).unwrap();
    assert_eq!(groups.groups().iter().map(|group| group.id).collect::<Vec<_>>(), [1, 2], "main ids stay small");
    assert_eq!(groups.split(db, None, SplitDirection::Right, false), None, "a workspace's group does not split");
}

#[test]
fn new_tabs_never_go_into_an_owned_group_unless_named() {
    let mut groups = opened(&["a.go"]);
    groups.split(1, None, SplitDirection::Right, false).unwrap();
    groups.focus(1);
    let db = groups.own("database");
    groups.open_in(&path("console.sql"), db);
    assert_eq!(groups.expanded(), Some("database"));

    groups.open(&path("b.go"));
    assert_eq!(tabs(&groups), [vec!["a.go", "b.go"], vec!["a.go"]], "into the last focused main group");
    assert_eq!(owned_tabs(&groups, "database"), ["console.sql"]);
    assert_eq!(groups.focused(), 1, "a code file takes the keyboard back to the main editor");
    assert_eq!(groups.expanded(), None);

    groups.open_in(&path("query.sql"), db);
    groups.select(db, 0);
    groups.focus(2);
    groups.open(&path("query.sql"));
    assert_eq!(groups.focused(), db, "a file shown in an owned group comes to front there");
    assert_eq!(active(&groups).as_deref(), Some("query.sql"));
    assert_eq!(tabs(&groups), [vec!["a.go", "b.go"], vec!["a.go"]], "and gets no main tab");
    assert_eq!(groups.main_focused(), 2);

    groups.open(&path("a.go"));
    assert_eq!(groups.focused(), 2, "a file of the main editor focuses its group");
}

#[test]
fn a_tab_moves_between_an_owned_group_and_a_main_group_when_the_caller_allows_it() {
    let mut groups = opened(&["a.go", "b.go"]);
    let db = groups.own("database");
    groups.open_in(&path("console.sql"), db);
    groups.open_in(&path("other.sql"), db);
    assert!(groups.crosses_workspaces(1, db));
    assert!(groups.move_tab(db, 1, 1, None), "a script goes to the code editor");
    assert_eq!(tabs(&groups), [vec!["a.go", "b.go", "other.sql"]]);
    assert_eq!(groups.focused(), 1, "which takes the keyboard");
    assert!(groups.move_tab(1, 2, db, None), "and back");
    assert_eq!(tabs(&groups), [vec!["a.go", "b.go"]]);
    assert_eq!(owned_tabs(&groups, "database"), ["console.sql", "other.sql"]);
    assert_eq!(groups.expanded(), Some("database"));
    assert!(groups.reorder(db, 1, 0), "the tabs of a group are reordered in place");
    assert_eq!(owned_tabs(&groups, "database"), ["other.sql", "console.sql"]);
    assert_eq!(active(&groups).as_deref(), Some("other.sql"), "the active tab stays active");
    assert!(!groups.reorder(db, 5, 0));
}

#[test]
fn the_splitter_and_unsplit_walk_only_the_main_groups() {
    let mut groups = opened(&["a.go", "b.go"]);
    groups.split(1, None, SplitDirection::Right, false).unwrap();
    let db = groups.own("database");
    groups.open_in(&path("console.sql"), db);
    assert!(groups.focus_next(true), "from the workspace, the next splitter of the main editor");
    assert_eq!(groups.focused(), 1, "after group 2, which had the keyboard last");
    groups.focus(db);
    assert!(groups.focus_next(false));
    assert_eq!(groups.focused(), 2, "before group 1, which had the keyboard last");
    groups.focus(db);
    assert!(!groups.unsplit_all(), "from a workspace whose area is not split, nothing to unsplit");
    groups.focus(1);
    assert!(groups.unsplit_all());
    assert_eq!(tabs(&groups), [vec!["a.go", "b.go"]]);
    groups.focus(db);
    assert_eq!(groups.focused(), db);
    assert_eq!(owned_tabs(&groups, "database"), ["console.sql"]);
    groups.retain(|path| path != Path::new("/p/console.sql"));
    assert_eq!(groups.owned("database").map(|group| group.tabs.len()), Some(0), "files that are gone leave it");
    groups.relocate(Path::new("/p/b.go"), Path::new("/p/c.go"));
    assert_eq!(tabs(&groups), [vec!["a.go", "c.go"]]);
}

#[test]
fn restoring_keeps_owned_ids_and_older_files_load() {
    let mut groups = opened(&["a.go", "gone.go"]);
    groups.split(1, Some(1), SplitDirection::Right, true).unwrap();
    let db = groups.own("database");
    groups.open_in(&path("console.sql"), db);
    let json = serde_json::to_value(&groups).unwrap();
    assert_eq!(json["workspaces"][0]["workspace"], "database");
    assert_eq!(json["workspaces"][0]["root"]["id"], db);
    assert_eq!(json["workspaces"][0]["focused"], db);
    assert_eq!(json["mainFocused"], 2);
    let read: EditorGroups = serde_json::from_value(json).unwrap();
    let restored = read.restored(|path| path != Path::new("/p/a.go"));
    assert_eq!(tabs(&restored), [vec!["gone.go"]], "main groups renumber");
    assert_eq!(restored.groups()[0].id, 1);
    assert_eq!(restored.owned("database").unwrap().id, db, "an owned group keeps its id");
    assert_eq!(restored.focused(), db, "and the keyboard it had");
    assert_eq!(restored.main_focused(), 1);

    let older: EditorGroups = serde_json::from_value(json!({
        "focused": 1,
        "root": { "kind": "group", "id": 1, "tabs": ["/p/a.go"], "active": 0 }
    }))
    .unwrap();
    let older = older.restored(|_| true);
    assert!(older.owned_groups().is_empty());
    assert_eq!((older.focused(), older.main_focused()), (1, 1));
    assert!(serde_json::to_value(&older).unwrap().get("workspaces").is_none(), "nothing written without workspaces");

    let doubled: EditorGroups = serde_json::from_value(json!({
        "focused": 1,
        "root": { "kind": "group", "id": 1, "tabs": [] },
        "workspaces": [
            { "workspace": "database", "root": { "kind": "group", "id": OWNED_FIRST, "tabs": [] }, "focused": OWNED_FIRST },
            { "workspace": "database", "root": { "kind": "group", "id": OWNED_FIRST + 1, "tabs": [] }, "focused": OWNED_FIRST + 1 },
            { "workspace": "git", "root": { "kind": "group", "id": 3, "tabs": [] }, "focused": 3 }
        ]
    }))
    .unwrap();
    let doubled = doubled.restored(|_| true);
    assert_eq!(doubled.owned_groups().len(), 1, "a doubled or misnumbered entry drops itself");
    assert_eq!(doubled.main_only().owned_groups().len(), 0);
}

#[test]
fn file_document_open_names_an_owned_group_and_split_list_reports_it() {
    let (registry, mut session, dir) = session();
    let db = workbench(&mut session).groups.own("database");
    let console = dir.path().join("console.sql");
    std::fs::write(&console, "select 1;\n").unwrap();
    registry.dispatch(&mut session, "file.document.open", json!({ "path": console, "group": db })).unwrap();
    assert_eq!(workbench(&mut session).groups().focused(), db);
    assert_eq!(workbench(&mut session).active_document().unwrap().path(), console);
    let listed = registry.dispatch(&mut session, "editor.split.list", json!({})).unwrap();
    assert_eq!(listed["workspaces"][0]["workspace"], "database");
    assert_eq!(listed["workspaces"][0]["root"]["tabs"], json!([console]));
    assert_eq!(listed["root"]["tabs"], json!([dir.path().join("a.go"), dir.path().join("b.go")]), "no main tab");

    let refused = registry
        .dispatch(&mut session, "editor.split.moveTab", json!({ "from": db, "index": 0, "to": 1 }))
        .unwrap_err();
    let zen_core::DispatchError::Failed { error, .. } = &refused else { panic!("{refused}") };
    assert_eq!(error.code.as_deref(), Some("notMovable"), "a workspace that takes no files lets none go: {refused}");
    assert!(registry.dispatch(&mut session, "file.document.open", json!({ "path": console, "group": 99 })).is_err());

    registry.dispatch(&mut session, "file.document.open", json!({ "path": dir.path().join("a.go") })).unwrap();
    assert_eq!(workbench(&mut session).groups().focused(), 1, "a code file collapses the workspace");
    registry.dispatch(&mut session, "file.document.open", json!({ "path": console })).unwrap();
    assert_eq!(workbench(&mut session).groups().focused(), db, "the console comes to front where it is");
    registry.dispatch(&mut session, "file.document.close", json!({})).unwrap();
    assert_eq!(workbench(&mut session).groups().focused(), db, "the emptied workspace group stays");
    assert!(workbench(&mut session).active_document().is_none());
    assert_eq!(workbench(&mut session).documents().len(), 2);
}

#[test]
fn pinned_tabs_keep_their_pin_through_dragging_unsplitting_and_a_restart() {
    let mut groups = opened(&["a.go", "b.go", "c.go"]);
    assert_eq!(groups.pin(1, 2, None), Some(true));
    assert_eq!(tabs(&groups), [vec!["c.go", "a.go", "b.go"]]);
    groups.reorder(1, 2, 0);
    assert_eq!(tabs(&groups), [vec!["c.go", "b.go", "a.go"]], "an unpinned tab never goes before a pinned one");
    groups.reorder(1, 0, 2);
    assert_eq!(tabs(&groups), [vec!["c.go", "b.go", "a.go"]], "a pinned tab never goes after an unpinned one");
    groups.split(1, Some(0), SplitDirection::Right, true);
    let right = groups.focused();
    groups.pin(right, 0, Some(true));
    groups.open_in(&path("d.go"), right);
    groups.focus(1);
    assert!(groups.unsplit());
    assert_eq!(tabs(&groups), [vec!["c.go", "b.go", "a.go", "d.go"]], "the other group's pins come along");
    assert!(groups.focused_group().is_pinned(&path("c.go")));
    assert!(groups.has_pinned());
    let saved: EditorGroups = serde_json::from_value(serde_json::to_value(&groups).unwrap()).unwrap();
    let restored = saved.restored(|_| true);
    assert_eq!(restored.focused_group().pinned, [path("c.go")]);
    assert_eq!(groups.pin(1, 0, None), Some(false));
    assert!(!groups.has_pinned());
}

fn file_names(tabs: &[PathBuf]) -> Vec<String> {
    tabs.iter().map(|tab| tab.file_name().unwrap().to_string_lossy().into_owned()).collect()
}

const BOUNDS: WindowBounds = WindowBounds { x: 10.0, y: 20.0, width: 640.0, height: 480.0 };

#[test]
fn a_tab_moves_into_a_new_window_with_the_focus() {
    let mut groups = opened(&["a.go", "b.go"]);
    let window = groups.detach(1, 1, Some(BOUNDS)).unwrap();
    assert_eq!(window, WINDOW_FIRST);
    assert_eq!(tabs(&groups), [vec!["a.go"]], "it left the main window");
    assert_eq!(file_names(&groups.window(window).unwrap().group.tabs), ["b.go"]);
    assert_eq!(groups.window(window).unwrap().bounds, Some(BOUNDS));
    assert_eq!(groups.focused(), window);
    assert_eq!(groups.main_focused(), 1, "new tabs still go to the main window");
    assert_eq!(active(&groups).as_deref(), Some("b.go"));
    assert_eq!(groups.detach(window, 0, None), None, "a window's only tab has nowhere better to go");
    assert_eq!(groups.detach(1, 0, None), Some(WINDOW_FIRST + 1), "the main window may end up empty");
    assert_eq!(tabs(&groups), [Vec::<String>::new()]);
}

#[test]
fn closing_the_last_tab_closes_the_window_and_moving_it_out_docks_it() {
    let mut groups = opened(&["a.go", "b.go", "c.go"]);
    let first = groups.detach(1, 2, None).unwrap();
    groups.close(first, 0);
    assert!(groups.windows().is_empty());
    assert_eq!(groups.focused(), 1, "the focus goes back");
    let second = groups.detach(1, 1, None).unwrap();
    assert!(groups.move_tab(second, 0, 1, Some(0)));
    assert!(groups.windows().is_empty(), "docking its last tab closes it");
    assert_eq!(tabs(&groups), [vec!["b.go", "a.go"]]);
    assert_eq!(groups.focused(), 1);
}

#[test]
fn closing_a_window_gives_its_tabs_back_to_the_main_window() {
    let mut groups = opened(&["a.go", "b.go"]);
    let window = groups.detach(1, 1, None).unwrap();
    assert!(groups.close_window(window));
    assert_eq!(tabs(&groups), [vec!["a.go", "b.go"]]);
    assert_eq!(groups.focused(), 1);
    assert!(!groups.close_window(window));
}

#[test]
fn opening_a_new_file_from_a_window_uses_the_main_window_and_a_file_it_holds_stays() {
    let mut groups = opened(&["a.go", "b.go"]);
    let window = groups.detach(1, 1, None).unwrap();
    groups.open(&path("b.go"));
    assert_eq!(groups.focused(), window, "the window already shows it");
    groups.open(&path("c.go"));
    assert_eq!(groups.focused(), 1);
    assert_eq!(tabs(&groups), [vec!["a.go", "c.go"]]);
    assert!(groups.contains(&path("b.go")));
}

#[test]
fn windows_survive_saving_with_their_bounds_and_lose_files_that_are_gone() {
    let mut groups = opened(&["a.go", "b.go", "c.go"]);
    let first = groups.detach(1, 1, None).unwrap();
    groups.detach(1, 1, None).unwrap();
    assert!(groups.set_window_bounds(first, BOUNDS));
    assert!(!groups.set_window_bounds(first, BOUNDS), "the same place is no change");
    let saved = serde_json::to_string(&groups).unwrap();
    let read: EditorGroups = serde_json::from_str(&saved).unwrap();
    assert_eq!(read, groups);
    let restored = read.restored(|file| file != path("c.go"));
    assert_eq!(restored.windows().len(), 1, "a window without files stays closed");
    assert_eq!(restored.windows()[0].bounds, Some(BOUNDS));
    assert!(restored.group(restored.focused()).is_some());
    assert_eq!(restored.main_only().windows().len(), 0);
}

#[test]
fn saved_windows_take_their_tabs_back_from_the_main_window() {
    let mut groups = opened(&["a.go", "b.go", "c.go"]);
    let window = groups.detach(1, 1, Some(BOUNDS)).unwrap();
    let saved = groups.windows().to_vec();
    let opened_again = opened(&["a.go", "b.go", "c.go"]);
    let restored = opened_again.with_windows(saved, |file| file != path("gone.go"));
    assert_eq!(tabs(&restored), [vec!["a.go", "c.go"]]);
    assert_eq!(restored.window(window).unwrap().bounds, Some(BOUNDS));
    let empty = opened(&["a.go"]).with_windows(groups.windows().to_vec(), |file| file == path("a.go"));
    assert!(empty.windows().is_empty(), "a window whose files are gone stays closed");
}

#[test]
fn the_window_commands_share_the_document_with_the_main_window() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.txt");
    std::fs::write(&file, "one").unwrap();
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).ok();
    registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
    registry.dispatch(&mut session, "editor.window.openTab", json!({})).unwrap();
    let listed = registry.dispatch(&mut session, "editor.window.list", json!({})).unwrap();
    let group = listed["windows"][0]["group"]["id"].as_u64().unwrap();
    let workbench = session.state_mut::<Workbench>();
    assert_eq!(workbench.documents().len(), 1, "one document, whichever window shows it");
    assert_eq!(workbench.active_document().unwrap().path(), file);
    let bounds = json!({ "group": group, "bounds": { "x": 1.0, "y": 2.0, "width": 300.0, "height": 200.0 } });
    registry.dispatch(&mut session, "editor.window.setBounds", bounds).unwrap();
    let listed = registry.dispatch(&mut session, "editor.window.list", json!({})).unwrap();
    assert_eq!(listed["windows"][0]["bounds"]["width"], 300.0);
    registry.dispatch(&mut session, "editor.window.close", json!({ "group": group })).unwrap();
    let workbench = session.state::<Workbench>().unwrap();
    assert!(workbench.groups().windows().is_empty());
    assert_eq!(workbench.groups().groups()[0].tabs, std::slice::from_ref(&file));
    assert!(registry.dispatch(&mut session, "editor.window.close", json!({ "group": group })).is_err());
}

#[test]
fn from_a_window_a_file_opens_there_when_the_setting_is_off() {
    let dir = tempfile::tempdir().unwrap();
    let (first, second) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&first, "a").unwrap();
    std::fs::write(&second, "b").unwrap();
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
    registry.dispatch(&mut session, "file.document.open", json!({ "path": first })).unwrap();
    registry.dispatch(&mut session, "editor.window.openTab", json!({})).unwrap();
    session.settings_mut().set(super::OPEN_IN_MAIN_WINDOW, false).unwrap();
    registry.dispatch(&mut session, "file.document.open", json!({ "path": second })).unwrap();
    let workbench = session.state::<Workbench>().unwrap();
    let window = workbench.groups().focused();
    assert!(workbench.groups().window(window).unwrap().group.tabs.contains(&second));
}
