use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use zen_core::{Registry, Session};

use super::super::testing::*;
use super::*;

fn diverged() -> (tempfile::TempDir, PathBuf) {
    let dir = test_repository();
    let root = dir.path().canonicalize().unwrap();
    git(&root, &["switch", "-q", "-c", "feature"]);
    std::fs::write(root.join("a.txt"), "one\nfeature\nthree\n").unwrap();
    git(&root, &["commit", "-q", "-am", "Feature"]);
    git(&root, &["switch", "-q", "main"]);
    std::fs::write(root.join("a.txt"), "one\nmain\nthree\nfour\n").unwrap();
    git(&root, &["commit", "-q", "-am", "Main"]);
    (dir, root)
}

fn run(registry: &Registry, session: &mut Session, command: &str, params: Value) -> Result<Value, CommandError> {
    registry.dispatch(session, command, params).map_err(|error| match error {
        zen_core::DispatchError::Failed { error, .. } => error,
        other => panic!("{command}: {other}"),
    })
}

fn read(root: &Path, file: &str) -> String {
    std::fs::read_to_string(root.join(file)).unwrap()
}

#[test]
fn a_merge_conflict_is_resolved_change_by_change_and_continued_into_one_undoable_merge() {
    let (_dir, root) = diverged();
    let (registry, mut session) = super::super::tests::session(&root);
    let s = &mut session;
    let error = run(&registry, s, "vcs.branch.merge", json!({ "name": "feature" })).unwrap_err();
    assert_eq!(error.code.as_deref(), Some(CONFLICTS));

    let list = run(&registry, s, "vcs.conflict.list", json!({})).unwrap();
    assert_eq!(list["operation"]["kind"], "merge");
    assert_eq!(list["operation"]["title"], t!("vcs-operation-merge", theirs = "feature", ours = "main"));
    assert_eq!(
        (list["operation"]["ours"].clone(), list["operation"]["theirs"].clone()),
        (json!("main"), json!("feature"))
    );
    assert_eq!(list["operation"]["canContinue"], json!(false));
    assert_eq!(list["files"][0]["relative"], "a.txt");
    assert_eq!(list["files"][0]["kind"], "bothModified");

    let view = run(&registry, s, "vcs.conflict.open", json!({})).unwrap();
    assert_eq!(s.state::<ConflictEditor>().map(ConflictEditor::raised), Some(1), "the tab comes to the front");
    assert_eq!(view["open"], json!(true));
    assert_eq!(view["mergeable"], json!(true));
    assert_eq!(view["ours"]["text"], "one\nmain\nthree\nfour\n");
    assert_eq!(view["theirs"]["text"], "one\nfeature\nthree\n");
    assert_eq!(view["base"], "one\ntwo\nthree\n");
    assert_eq!(view["result"], "one\ntwo\nthree\nfour\n", "the base's lines where they conflict");
    let changes = view["changes"].as_array().unwrap();
    let states: Vec<(&str, &str)> =
        changes.iter().map(|c| (c["kind"].as_str().unwrap(), c["state"].as_str().unwrap())).collect();
    assert_eq!(states, [("conflict", "unresolved"), ("ours", "auto")]);
    assert_eq!(changes[0]["ours"], json!([1, 2]));
    assert_eq!(changes[0]["result"], json!([1, 2]));
    assert_eq!((view["conflicts"].clone(), view["unresolved"].clone()), (json!(1), json!(1)));

    let refused = run(&registry, s, "vcs.conflict.resolve", json!({ "markResolved": true })).unwrap_err();
    assert_eq!(refused.code.as_deref(), Some(UNRESOLVED));
    let view = run(&registry, s, "vcs.conflict.resolve", json!({ "change": 0, "take": "both" })).unwrap();
    assert_eq!(view["result"], "one\nmain\nfeature\nthree\nfour\n");
    assert_eq!(view["unresolved"], json!(0));
    let view =
        run(&registry, s, "vcs.conflict.resolve", json!({ "text": "one\nmain and feature\nthree\nfour\n" })).unwrap();
    assert_eq!(view["changes"][0]["state"], "edited");
    assert_eq!(view["edited"], json!(true));
    assert!(read(&root, "a.txt").contains("<<<<<<<"), "nothing written yet");
    let view = run(&registry, s, "vcs.conflict.resolve", json!({ "markResolved": true })).unwrap();
    assert_eq!((view["resolved"].clone(), view["remaining"].clone()), (json!(true), json!(0)));
    assert!(!s.state::<ConflictEditor>().unwrap().is_open(), "the editor closes");
    assert_eq!(read(&root, "a.txt"), "one\nmain and feature\nthree\nfour\n");
    let list = run(&registry, s, "vcs.conflict.list", json!({})).unwrap();
    assert_eq!(list["operation"]["canContinue"], json!(true));

    let done = run(&registry, s, "vcs.operation.continue", json!({})).unwrap();
    assert_eq!(done["operation"], "merge");
    assert_eq!(git(&root, &["log", "-1", "--format=%s"]).trim(), "Merge branch 'feature'");
    assert_eq!(run(&registry, s, "vcs.conflict.list", json!({})).unwrap()["operation"], Value::Null);
    let history = run(&registry, s, "vcs.history.list", json!({})).unwrap();
    assert_eq!(history["undo"]["title"], t!("vcs-history-op-merge", subject = "feature", target = "main"));
    run(&registry, s, "vcs.history.undo", json!({})).unwrap();
    assert_eq!(git(&root, &["log", "-1", "--format=%s"]).trim(), "Main");
    assert!(run(&registry, s, "vcs.operation.continue", json!({})).is_err(), "nothing in progress");
}

#[test]
fn a_cherry_pick_takes_a_whole_side_and_a_revert_aborts() {
    let (_dir, root) = diverged();
    let (registry, mut session) = super::super::tests::session(&root);
    let s = &mut session;
    let feature = git(&root, &["rev-parse", "feature"]).trim().to_owned();
    let main = git(&root, &["rev-parse", "main"]).trim().to_owned();
    assert!(run(&registry, s, "vcs.commit.cherryPick", json!({ "id": feature })).is_err());
    let list = run(&registry, s, "vcs.conflict.list", json!({})).unwrap();
    assert_eq!(list["operation"]["kind"], "cherryPick");
    assert_eq!(list["operation"]["theirs"], format!("{} Feature", &feature[..7]));
    let view = run(&registry, s, "vcs.conflict.resolve", json!({ "path": "a.txt", "take": "theirs" })).unwrap();
    assert_eq!((view["resolved"].clone(), view["open"].clone()), (json!(true), json!(false)));
    assert_eq!(read(&root, "a.txt"), "one\nfeature\nthree\n");
    run(&registry, s, "vcs.operation.continue", json!({})).unwrap();
    assert_eq!(git(&root, &["log", "-1", "--format=%s"]).trim(), "Feature");
    let history = run(&registry, s, "vcs.history.list", json!({})).unwrap();
    assert_eq!(history["undo"]["operation"], "cherryPick");

    let head = git(&root, &["rev-parse", "HEAD"]);
    assert!(run(&registry, s, "vcs.commit.revert", json!({ "id": main })).is_err());
    assert_eq!(run(&registry, s, "vcs.conflict.list", json!({})).unwrap()["operation"]["kind"], "revert");
    run(&registry, s, "vcs.conflict.open", json!({ "path": root.join("a.txt") })).unwrap();
    let done = run(&registry, s, "vcs.operation.abort", json!({})).unwrap();
    assert_eq!(done["operation"], "revert");
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), head);
    assert!(!s.state::<ConflictEditor>().unwrap().is_open(), "nothing conflicted: the editor closes");
    let history = run(&registry, s, "vcs.history.list", json!({})).unwrap();
    assert_eq!(history["undo"]["operation"], "cherryPick", "an aborted revert is not recorded");
}

#[test]
fn a_rebase_continues_through_the_same_command() {
    let (_dir, root) = diverged();
    let (registry, mut session) = super::super::tests::session(&root);
    let s = &mut session;
    git(&root, &["switch", "-q", "feature"]);
    assert!(run(&registry, s, "vcs.branch.rebase", json!({ "onto": "main" })).is_err());
    let list = run(&registry, s, "vcs.conflict.list", json!({})).unwrap();
    let operation = &list["operation"];
    assert_eq!(operation["kind"], "rebase");
    assert_eq!((operation["step"].clone(), operation["total"].clone()), (json!(1), json!(1)));
    assert_eq!(operation["branch"], "feature");
    let view = run(&registry, s, "vcs.conflict.open", json!({ "path": "a.txt" })).unwrap();
    assert_eq!(view["ours"]["text"], "one\nmain\nthree\nfour\n");
    assert_eq!(view["operation"], "rebase");
    run(&registry, s, "vcs.conflict.resolve", json!({ "conflicts": true, "take": "theirs", "markResolved": true }))
        .unwrap();
    assert_eq!(read(&root, "a.txt"), "one\nfeature\nthree\nfour\n");
    run(&registry, s, "vcs.operation.continue", json!({})).unwrap();
    assert_eq!(git(&root, &["log", "--format=%s"]).lines().collect::<Vec<_>>(), ["Feature", "Main", "First"]);
    let history = run(&registry, s, "vcs.history.list", json!({})).unwrap();
    assert_eq!(history["undo"]["operation"], "rebase");
}

#[test]
fn a_stash_that_conflicts_is_aborted_or_continued_and_dropped() {
    let dir = test_repository();
    let root = dir.path().canonicalize().unwrap();
    let (registry, mut session) = super::super::tests::session(&root);
    let s = &mut session;
    std::fs::write(root.join("a.txt"), "one\nstashed\nthree\n").unwrap();
    std::fs::write(root.join("b.txt"), "b\n").unwrap();
    git(&root, &["add", "b.txt"]);
    run(&registry, s, "vcs.stash.create", json!({})).unwrap();
    std::fs::write(root.join("a.txt"), "one\ncommitted\nthree\n").unwrap();
    git(&root, &["commit", "-q", "-am", "Second"]);
    let other = "other\n";
    std::fs::write(root.join("other.txt"), other).unwrap();

    assert!(run(&registry, s, "vcs.stash.pop", json!({})).is_err());
    let list = run(&registry, s, "vcs.conflict.list", json!({})).unwrap();
    assert_eq!(list["operation"]["kind"], "stashPop");
    assert_eq!(list["operation"]["theirs"], "stash@{0}");
    assert_eq!(list["files"][0]["relative"], "a.txt");
    assert!(root.join("b.txt").exists());
    run(&registry, s, "vcs.operation.abort", json!({})).unwrap();
    assert_eq!(read(&root, "a.txt"), "one\ncommitted\nthree\n");
    assert!(!root.join("b.txt").exists(), "a file the stash added goes again");
    assert_eq!(read(&root, "other.txt"), other, "files it did not touch are left alone");
    assert_eq!(git(&root, &["status", "--porcelain"]).trim(), "?? other.txt");
    assert_eq!(git(&root, &["stash", "list"]).lines().count(), 1);
    assert_eq!(run(&registry, s, "vcs.conflict.list", json!({})).unwrap()["operation"], Value::Null);

    assert!(run(&registry, s, "vcs.stash.pop", json!({})).is_err());
    let refused = run(&registry, s, "vcs.operation.continue", json!({})).unwrap_err();
    assert_eq!(refused.code.as_deref(), Some(CONFLICTS), "still conflicted");
    run(&registry, s, "vcs.conflict.resolve", json!({ "path": "a.txt", "take": "ours" })).unwrap();
    let done = run(&registry, s, "vcs.operation.continue", json!({})).unwrap();
    assert_eq!(done["operation"], "stashPop");
    assert_eq!(git(&root, &["stash", "list"]).trim(), "", "popped: dropped");
    assert_eq!(read(&root, "a.txt"), "one\ncommitted\nthree\n");
    assert!(root.join("b.txt").exists());
    assert_eq!(run(&registry, s, "vcs.conflict.list", json!({})).unwrap()["operation"], Value::Null);
}

#[test]
fn a_file_deleted_on_one_side_is_taken_whole_and_a_commit_finishes_the_merge() {
    let dir = test_repository();
    let root = dir.path().canonicalize().unwrap();
    git(&root, &["switch", "-q", "-c", "feature"]);
    git(&root, &["rm", "-q", "a.txt"]);
    git(&root, &["commit", "-q", "-m", "Delete"]);
    git(&root, &["switch", "-q", "main"]);
    std::fs::write(root.join("a.txt"), "one\nchanged\nthree\n").unwrap();
    git(&root, &["commit", "-q", "-am", "Change"]);
    let (registry, mut session) = super::super::tests::session(&root);
    let s = &mut session;
    assert!(run(&registry, s, "vcs.branch.merge", json!({ "name": "feature" })).is_err());
    let list = run(&registry, s, "vcs.conflict.list", json!({})).unwrap();
    assert_eq!(list["files"][0]["kind"], "deletedByThem");
    let view = run(&registry, s, "vcs.conflict.open", json!({})).unwrap();
    assert_eq!(view["mergeable"], json!(false));
    assert_eq!((view["ours"]["exists"].clone(), view["theirs"]["exists"].clone()), (json!(true), json!(false)));
    assert!(run(&registry, s, "vcs.conflict.resolve", json!({ "change": 0, "take": "ours" })).is_err());
    assert!(run(&registry, s, "vcs.conflict.resolve", json!({ "take": "both" })).is_err());
    run(&registry, s, "vcs.conflict.resolve", json!({ "take": "theirs" })).unwrap();
    assert!(!root.join("a.txt").exists());
    run(&registry, s, "vcs.commit.create", json!({ "message": "Merge feature", "staged": true })).unwrap();
    assert_eq!(git(&root, &["rev-list", "--parents", "-1", "HEAD"]).split_whitespace().count(), 3);
    let history = run(&registry, s, "vcs.history.list", json!({})).unwrap();
    assert_eq!(history["undo"]["operation"], "merge");
    assert!(run(&registry, s, "vcs.conflict.get", json!({})).is_err(), "nothing open, nothing named");
}
