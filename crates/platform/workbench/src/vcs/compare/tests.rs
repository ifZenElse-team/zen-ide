use std::path::PathBuf;

use serde_json::{Value, json};
use zen_core::{Registry, Session};

use super::super::testing::*;
use super::*;

fn session(root: &Path) -> (Registry, Session) {
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
    (registry, session)
}

fn run(registry: &Registry, session: &mut Session, command: &str, params: Value) -> Result<Value, CommandError> {
    registry.dispatch(session, command, params).map_err(|error| match error {
        zen_core::DispatchError::Failed { error, .. } => error,
        other => panic!("{command}: {other}"),
    })
}

fn commit(root: &Path, subject: &str) -> String {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", subject]);
    git(root, &["rev-parse", "HEAD"]).trim().to_owned()
}

fn history() -> (tempfile::TempDir, PathBuf, Vec<String>) {
    let dir = test_repository();
    let root = dir.path().canonicalize().unwrap();
    let first = git(&root, &["rev-parse", "HEAD"]).trim().to_owned();
    std::fs::write(root.join("b.txt"), "bee\n").unwrap();
    let second = commit(&root, "Add b");
    std::fs::write(root.join("a.txt"), "one\nTWO\nthree\n").unwrap();
    let third = commit(&root, "Shout two");
    git(&root, &["mv", "a.txt", "c.txt"]);
    std::fs::write(root.join("c.txt"), "one\nTWO\nthree\nfour\n").unwrap();
    let fourth = commit(&root, "Rename a to c");
    (dir, root, vec![first, second, third, fourth])
}

fn files(compared: &Value) -> Vec<(String, String)> {
    compared["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| (file["relative"].as_str().unwrap().to_owned(), file["status"].as_str().unwrap().to_owned()))
        .collect()
}

#[test]
fn two_commits_compare_by_their_differing_files_and_each_files_diff() {
    let (_dir, root, ids) = history();
    let (registry, mut session) = session(&root);
    let compared = run(&registry, &mut session, "vcs.diff.commits", json!({ "from": ids[0], "to": ids[2] })).unwrap();
    assert_eq!(files(&compared), [("a.txt".into(), "modified".into()), ("b.txt".into(), "added".into())]);
    assert_eq!(compared["from"], json!(ids[0]));
    assert_eq!(compared["to"], json!(ids[2]));
    assert_eq!(compared["files"][0]["path"], json!(root.join("a.txt")));
    let params = json!({ "from": ids[1], "to": "main", "path": "c.txt" });
    let compared = run(&registry, &mut session, "vcs.diff.commits", params).unwrap();
    assert_eq!(compared["files"][0]["oldPath"], json!("a.txt"));
    assert_eq!(compared["files"][0]["status"], json!("renamed"));
    let diff = &compared["diff"];
    assert_eq!(diff["before"], json!("one\ntwo\nthree\n"));
    assert_eq!(diff["after"], json!("one\nTWO\nthree\nfour\n"));
    assert_eq!(diff["beforeTitle"], json!(&ids[1][..8]));
    assert_eq!(diff["afterTitle"], json!(&ids[3][..8]));
    let params = json!({ "id": ids[3], "base": ids[1], "path": root.join("c.txt") });
    let opened = run(&registry, &mut session, "vcs.workspace.openDiff", params).unwrap();
    assert_eq!(opened["before"], json!("one\ntwo\nthree\n"));
    assert!(session.state_mut::<GitWorkspace>().take_diff().is_some(), "the window opens it");
    assert!(run(&registry, &mut session, "vcs.diff.commits", json!({ "from": "" })).is_err());
    assert!(run(&registry, &mut session, "vcs.diff.commits", json!({ "from": "nope" })).is_err());
}

#[test]
fn a_commit_compares_with_the_work_in_progress() {
    let (_dir, root, ids) = history();
    let (registry, mut session) = session(&root);
    std::fs::write(root.join("b.txt"), "bee\nhive\n").unwrap();
    std::fs::write(root.join("new.txt"), "new\n").unwrap();
    let compared = run(&registry, &mut session, "vcs.diff.commits", json!({ "from": ids[3] })).unwrap();
    assert_eq!(compared["to"], Value::Null, "the working tree");
    assert_eq!(files(&compared), [("b.txt".into(), "modified".into()), ("new.txt".into(), "added".into())]);
    let params = json!({ "from": ids[0], "path": "b.txt" });
    let compared = run(&registry, &mut session, "vcs.diff.commits", params).unwrap();
    assert!(files(&compared).contains(&("c.txt".into(), "renamed".into())));
    assert_eq!(compared["diff"]["before"], json!(""));
    assert_eq!(compared["diff"]["after"], json!("bee\nhive\n"));
    assert_eq!(compared["diff"]["afterTitle"], json!(t!("vcs-diff-yours")));
    run(&registry, &mut session, "file.document.open", json!({ "path": root.join("b.txt") })).unwrap();
    run(&registry, &mut session, "editor.edit.insert", json!({ "text": "x" })).unwrap();
    let params = json!({ "base": ids[3], "path": "b.txt" });
    let opened = run(&registry, &mut session, "vcs.workspace.openDiff", params).unwrap();
    assert_eq!(opened["after"], json!("xbee\nhive\n"));
}

#[test]
fn picking_two_points_with_command_click_compares_them_in_the_right_panel() {
    let (_dir, root, ids) = history();
    let (registry, mut session) = session(&root);
    std::fs::write(root.join("b.txt"), "changed\n").unwrap();
    run(&registry, &mut session, "vcs.workspace.open", json!({})).unwrap();
    run(&registry, &mut session, "vcs.workspace.select", json!({ "revision": ids[2] })).unwrap();
    run(&registry, &mut session, "vcs.workspace.select", json!({ "revision": ids[0], "toggle": true })).unwrap();
    let state = run(&registry, &mut session, "vcs.workspace.state", json!({})).unwrap();
    assert_eq!(state["picked"], json!([ids[2], ids[0]]));
    assert_eq!(state["panel"], json!({ "kind": "compare", "from": ids[0], "to": ids[2] }));
    run(&registry, &mut session, "vcs.workspace.menu", json!({ "revision": ids[2] })).unwrap();
    session.state_mut::<GitWorkspace>().panel = None;
    let params = json!({ "revision": ids[2], "action": "compare" });
    run(&registry, &mut session, "vcs.workspace.commitAction", params).unwrap();
    let state = run(&registry, &mut session, "vcs.workspace.state", json!({})).unwrap();
    assert_eq!(state["panel"]["kind"], json!("compare"));
    run(&registry, &mut session, "vcs.workspace.select", json!({ "revision": ids[1], "toggle": true })).unwrap();
    assert_eq!(run(&registry, &mut session, "vcs.workspace.state", json!({})).unwrap()["panel"], Value::Null);
    let params = json!({ "revision": ids[2], "action": "compare" });
    assert!(run(&registry, &mut session, "vcs.workspace.commitAction", params).is_err(), "three picked");
    run(&registry, &mut session, "vcs.workspace.select", json!({ "revision": ids[1], "toggle": true })).unwrap();
    assert_eq!(run(&registry, &mut session, "vcs.workspace.state", json!({})).unwrap()["panel"]["from"], json!(ids[0]));
    run(&registry, &mut session, "vcs.workspace.select", json!({ "revision": ids[1] })).unwrap();
    let state = run(&registry, &mut session, "vcs.workspace.state", json!({})).unwrap();
    assert_eq!((state["picked"].clone(), state["panel"].clone()), (json!([]), Value::Null));

    run(&registry, &mut session, "vcs.workspace.select", json!({ "wip": true, "toggle": true })).unwrap();
    let state = run(&registry, &mut session, "vcs.workspace.state", json!({})).unwrap();
    assert_eq!((state["picked"].clone(), state["pickedWip"].clone()), (json!([ids[1]]), json!(true)));
    assert_eq!(state["panel"], json!({ "kind": "compare", "from": ids[1], "to": null }));
    run(&registry, &mut session, "vcs.workspace.select", json!({ "wip": true })).unwrap();
    run(&registry, &mut session, "vcs.workspace.select", json!({ "revision": ids[3], "toggle": true })).unwrap();
    let state = run(&registry, &mut session, "vcs.workspace.state", json!({})).unwrap();
    assert_eq!(state["panel"], json!({ "kind": "compare", "from": ids[3], "to": null }));
    let params = json!({ "revision": ids[3], "action": "compare" });
    run(&registry, &mut session, "vcs.workspace.commitAction", params).unwrap();
    run(&registry, &mut session, "vcs.workspace.select", json!({ "wip": true, "toggle": true })).unwrap();
    let state = run(&registry, &mut session, "vcs.workspace.state", json!({})).unwrap();
    assert_eq!((state["pickedWip"].clone(), state["panel"].clone()), (json!(false), Value::Null));
    let params = json!({ "from": ids[0], "to": ids[1], "show": true });
    run(&registry, &mut session, "vcs.diff.commits", params).unwrap();
    let state = run(&registry, &mut session, "vcs.workspace.state", json!({})).unwrap();
    assert_eq!(state["panel"], json!({ "kind": "compare", "from": ids[0], "to": ids[1] }));
}

#[test]
fn a_files_history_lists_its_commits_through_a_rename() {
    let (_dir, root, ids) = history();
    let (registry, mut session) = session(&root);
    let history = run(&registry, &mut session, "vcs.file.history", json!({ "path": root.join("c.txt") })).unwrap();
    let listed: Vec<(&str, &str, &str)> = history["commits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (entry["id"].as_str().unwrap(), entry["relative"].as_str().unwrap(), entry["status"].as_str().unwrap())
        })
        .collect();
    assert_eq!(
        listed,
        [
            (ids[3].as_str(), "c.txt", "renamed"),
            (ids[2].as_str(), "a.txt", "modified"),
            (ids[0].as_str(), "a.txt", "added"),
        ]
    );
    assert_eq!(history["commits"][0]["oldPath"], json!("a.txt"));
    assert_eq!(history["commits"][0]["subject"], json!("Rename a to c"));
    assert_eq!(history["commits"][1]["path"], json!(root.join("a.txt")));
    assert_eq!(history["more"], json!(false));
    let page = run(&registry, &mut session, "vcs.file.history", json!({ "path": "c.txt", "limit": 1 })).unwrap();
    assert_eq!((page["commits"].as_array().unwrap().len(), page["more"].clone()), (1, json!(true)));
    let params = json!({ "id": ids[3], "base": ids[2], "path": "c.txt" });
    let diff = run(&registry, &mut session, "vcs.workspace.openDiff", params).unwrap();
    assert_eq!(diff["before"], json!("one\nTWO\nthree\n"));
    let diff =
        run(&registry, &mut session, "vcs.workspace.openDiff", json!({ "id": ids[2], "path": "a.txt" })).unwrap();
    assert_eq!(
        (diff["before"].clone(), diff["after"].clone()),
        (json!("one\ntwo\nthree\n"), json!("one\nTWO\nthree\n"))
    );
    assert!(run(&registry, &mut session, "vcs.file.history", json!({})).is_err(), "needs a file");
    run(&registry, &mut session, "vcs.file.history", json!({ "path": "c.txt", "show": true })).unwrap();
    let state = run(&registry, &mut session, "vcs.workspace.state", json!({})).unwrap();
    assert_eq!(state["open"], json!(true));
    assert_eq!(state["panel"], json!({ "kind": "fileHistory", "path": root.join("c.txt") }));
}

#[test]
fn a_files_menu_shows_its_history_and_opens_it_with_blame() {
    let (_dir, root, ids) = history();
    let (registry, mut session) = session(&root);
    run(&registry, &mut session, "vcs.workspace.open", json!({})).unwrap();
    let params = json!({ "path": "c.txt", "id": ids[3] });
    let state = run(&registry, &mut session, "vcs.workspace.fileMenu", params).unwrap();
    assert_eq!(state["fileMenu"], json!({ "path": root.join("c.txt"), "id": ids[3] }));
    let params = json!({ "path": root.join("c.txt"), "action": "history" });
    let done = run(&registry, &mut session, "vcs.workspace.fileAction", params).unwrap();
    assert_eq!(done["history"]["commits"].as_array().unwrap().len(), 3);
    let state = run(&registry, &mut session, "vcs.workspace.state", json!({})).unwrap();
    assert_eq!(state["fileMenu"], Value::Null, "the item closes the menu");
    assert_eq!(state["panel"]["kind"], json!("fileHistory"));
    run(&registry, &mut session, "vcs.workspace.select", json!({ "revision": ids[1] })).unwrap();
    assert_eq!(run(&registry, &mut session, "vcs.workspace.state", json!({})).unwrap()["panel"], Value::Null);

    let params = json!({ "path": "c.txt", "action": "blame" });
    run(&registry, &mut session, "vcs.workspace.fileAction", params).unwrap();
    let active =
        session.state::<crate::Workbench>().and_then(crate::Workbench::active_document).map(|d| d.path().to_owned());
    assert_eq!(active, Some(root.join("c.txt")));
    let blame = run(&registry, &mut session, "vcs.blame.get", json!({ "wait": true })).unwrap();
    assert_eq!(blame["shown"], json!(true));
    assert_eq!(blame["lines"][1]["commit"], json!(ids[2]), "TWO came with the third commit");
    assert!(session.state::<GitWorkspace>().unwrap().blamed_here(&root.join("c.txt")));
    run(&registry, &mut session, "vcs.workspace.fileAction", json!({ "path": "b.txt", "action": "open" })).unwrap();
    let active =
        session.state::<crate::Workbench>().and_then(crate::Workbench::active_document).map(|d| d.path().to_owned());
    assert_eq!(active, Some(root.join("b.txt")));
    let params = json!({ "path": "a.txt", "action": "blame" });
    assert!(run(&registry, &mut session, "vcs.workspace.fileAction", params).is_err(), "renamed away");
    run(&registry, &mut session, "vcs.workspace.fileMenu", json!({ "path": "b.txt" })).unwrap();
    let state = run(&registry, &mut session, "vcs.workspace.fileMenu", json!({})).unwrap();
    assert_eq!(state["fileMenu"], Value::Null);
}
