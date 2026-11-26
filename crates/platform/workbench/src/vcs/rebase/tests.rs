use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use zen_core::{Registry, Session};

use super::super::testing::*;
use super::*;

fn history() -> (tempfile::TempDir, PathBuf) {
    let dir = test_repository();
    let root = dir.path().canonicalize().unwrap();
    for (file, subject) in [("b.txt", "B"), ("c.txt", "C"), ("d.txt", "D")] {
        std::fs::write(root.join(file), format!("{subject}\n")).unwrap();
        git(&root, &["add", file]);
        git(&root, &["commit", "-q", "-m", subject]);
    }
    (dir, root)
}

fn rev(root: &Path, revision: &str) -> String {
    git(root, &["rev-parse", revision]).trim().to_owned()
}

fn subjects(root: &Path) -> Vec<String> {
    git(root, &["log", "--format=%s"]).lines().map(str::to_owned).collect()
}

fn run(registry: &Registry, session: &mut Session, command: &str, params: Value) -> Result<Value, CommandError> {
    registry.dispatch(session, command, params).map_err(|error| match error {
        zen_core::DispatchError::Failed { error, .. } => error,
        other => panic!("{command}: {other}"),
    })
}

fn step_subjects(plan: &Value) -> Vec<String> {
    plan["steps"].as_array().unwrap().iter().map(|step| step["subject"].as_str().unwrap().to_owned()).collect()
}

#[test]
fn keys_name_the_actions() {
    assert_eq!(RebaseAction::from_key('p'), Some(RebaseAction::Pick));
    assert_eq!(RebaseAction::from_key('R'), Some(RebaseAction::Reword));
    assert_eq!(RebaseAction::from_key('s'), Some(RebaseAction::Squash));
    assert_eq!(RebaseAction::from_key('f'), Some(RebaseAction::Fixup));
    assert_eq!(RebaseAction::from_key('d'), Some(RebaseAction::Drop));
    assert_eq!(RebaseAction::from_key('x'), None);
}

#[test]
fn a_plan_is_edited_reset_and_cancelled_without_touching_the_repository() {
    let (_dir, root) = history();
    let (registry, mut session) = super::super::tests::session(&root);
    let s = &mut session;
    let before = rev(&root, "HEAD");
    let first = rev(&root, "HEAD~3");
    let plan = run(&registry, s, "vcs.rebase.plan", json!({ "from": first })).unwrap();
    assert_eq!(step_subjects(&plan), ["B", "C", "D"], "the oldest first");
    assert_eq!(plan["kind"], "rebase");
    assert_eq!(plan["branch"], "main");
    assert_eq!(plan["checkout"], json!(false));
    assert!(plan["steps"].as_array().unwrap().iter().all(|step| step["action"] == "pick"));
    let state = run(&registry, s, "vcs.rebase.state", json!({})).unwrap();
    assert_eq!((state["open"].clone(), state["changed"].clone()), (json!(true), json!(false)));
    assert_eq!(s.state::<RebaseEditor>().map(RebaseEditor::raised), Some(1), "the tab comes to the front");

    let d = rev(&root, "HEAD");
    run(&registry, s, "vcs.rebase.edit", json!({ "index": 1, "action": "drop" })).unwrap();
    let plan = run(&registry, s, "vcs.rebase.edit", json!({ "id": &d[..8], "message": "D, again" })).unwrap();
    assert_eq!(plan["steps"][2]["action"], "reword");
    assert_eq!(plan["steps"][2]["message"], "D, again");
    let plan = run(&registry, s, "vcs.rebase.edit", json!({ "index": 2, "moveTo": 0 })).unwrap();
    assert_eq!(step_subjects(&plan), ["D", "B", "C"]);
    let plan = run(&registry, s, "vcs.rebase.edit", json!({ "index": 1, "action": "squash" })).unwrap();
    assert_eq!(plan["steps"][1]["action"], "squash");
    let plan = run(&registry, s, "vcs.rebase.edit", json!({ "index": 1, "action": "reword" })).unwrap();
    assert_eq!(plan["steps"][1]["message"], "B");
    assert!(run(&registry, s, "vcs.rebase.edit", json!({ "index": 9, "action": "drop" })).is_err());
    assert_eq!(run(&registry, s, "vcs.rebase.state", json!({})).unwrap()["changed"], json!(true));

    let plan = run(&registry, s, "vcs.rebase.reset", json!({})).unwrap();
    assert_eq!(step_subjects(&plan), ["B", "C", "D"]);
    assert!(plan["steps"].as_array().unwrap().iter().all(|step| step["action"] == "pick" && step["message"].is_null()));
    run(&registry, s, "vcs.rebase.cancel", json!({})).unwrap();
    assert_eq!(run(&registry, s, "vcs.rebase.state", json!({})).unwrap()["open"], json!(false));
    assert_eq!(rev(&root, "HEAD"), before, "nothing ran");
    assert!(run(&registry, s, "vcs.rebase.start", json!({})).is_err(), "no plan any more");

    assert!(run(&registry, s, "vcs.rebase.plan", json!({})).is_err());
    assert!(run(&registry, s, "vcs.rebase.plan", json!({ "from": "HEAD" })).is_err(), "nothing after HEAD");
    assert!(run(&registry, s, "vcs.rebase.plan", json!({ "onto": "main" })).is_err(), "nothing to rebase");
    assert!(run(&registry, s, "vcs.rebase.plan", json!({ "cherryPick": ["HEAD~1"] })).is_err(), "already here");
}

#[test]
fn a_started_rebase_rewrites_the_branch_and_undo_takes_it_back() {
    let (_dir, root) = history();
    let (registry, mut session) = super::super::tests::session(&root);
    let s = &mut session;
    let before = rev(&root, "HEAD");
    run(&registry, s, "vcs.workspace.open", json!({})).unwrap();
    run(&registry, s, "vcs.rebase.plan", json!({ "from": "HEAD~3" })).unwrap();
    run(&registry, s, "vcs.rebase.edit", json!({ "index": 0, "action": "squash" })).unwrap();
    let problem = run(&registry, s, "vcs.rebase.state", json!({})).unwrap()["problem"].clone();
    assert!(problem.as_str().is_some_and(|problem| problem.contains(&rev(&root, "HEAD~2")[..7])), "{problem}");
    assert!(run(&registry, s, "vcs.rebase.start", json!({})).is_err());
    assert_eq!(rev(&root, "HEAD"), before);
    run(&registry, s, "vcs.rebase.edit", json!({ "index": 0, "action": "pick" })).unwrap();
    std::fs::write(root.join("a.txt"), "changed\n").unwrap();
    let error = run(&registry, s, "vcs.rebase.start", json!({})).unwrap_err();
    assert!(error.message.contains("stash"), "{}", error.message);
    git(&root, &["checkout", "--", "a.txt"]);

    let (b, c, d) = (rev(&root, "HEAD~2"), rev(&root, "HEAD~1"), rev(&root, "HEAD"));
    let steps = json!([
        { "id": b, "action": "pick" },
        { "id": c, "action": "squash" },
        { "id": d, "action": "reword", "message": "D, reworded" },
    ]);
    let done = run(&registry, s, "vcs.rebase.start", json!({ "steps": steps })).unwrap();
    assert_eq!(subjects(&root), ["D, reworded", "B", "First"]);
    assert_eq!(done["head"], rev(&root, "HEAD"));
    assert_eq!(done["branch"], "main");
    let workspace = run(&registry, s, "vcs.workspace.state", json!({})).unwrap();
    assert_eq!(workspace["selected"], rev(&root, "HEAD"), "the new HEAD is selected");
    assert_eq!(run(&registry, s, "vcs.rebase.state", json!({})).unwrap()["open"], json!(false), "the editor closed");

    let list = run(&registry, s, "vcs.history.list", json!({})).unwrap();
    assert_eq!(list["undo"]["operation"], "interactiveRebase");
    assert_eq!(list["undo"]["title"], format!("Interactive Rebase 'main' onto '{}'", &rev(&root, "HEAD~2")[..7]));
    run(&registry, s, "vcs.history.undo", json!({})).unwrap();
    assert_eq!(rev(&root, "HEAD"), before);
    assert_eq!(subjects(&root), ["D", "C", "B", "First"]);
    run(&registry, s, "vcs.history.redo", json!({})).unwrap();
    assert_eq!(subjects(&root), ["D, reworded", "B", "First"]);
}

#[test]
fn a_branch_is_rebased_onto_another_from_the_branch_menu_and_the_drop() {
    let (_dir, root) = history();
    git(&root, &["switch", "-q", "-c", "feature", "HEAD~2"]);
    std::fs::write(root.join("f.txt"), "f\n").unwrap();
    git(&root, &["add", "f.txt"]);
    git(&root, &["commit", "-q", "-m", "F"]);
    let (registry, mut session) = super::super::tests::session(&root);
    let s = &mut session;
    run(&registry, s, "vcs.workspace.open", json!({})).unwrap();
    let opened =
        run(&registry, s, "vcs.workspace.refAction", json!({ "name": "main", "action": "interactiveRebase" })).unwrap();
    assert_eq!(opened["dialog"], "interactiveRebase");
    let plan = run(&registry, s, "vcs.rebase.state", json!({})).unwrap()["plan"].clone();
    assert_eq!(step_subjects(&plan), ["F"]);
    assert_eq!((plan["branch"].clone(), plan["checkout"].clone()), (json!("feature"), json!(false)));
    run(&registry, s, "vcs.rebase.start", json!({})).unwrap();
    assert_eq!(subjects(&root), ["F", "D", "C", "B", "First"]);
    assert!(
        run(&registry, s, "vcs.workspace.refAction", json!({ "name": "feature", "action": "interactiveRebase" }))
            .is_err()
    );

    git(&root, &["switch", "-q", "-c", "other", "HEAD~4"]);
    std::fs::write(root.join("o.txt"), "o\n").unwrap();
    git(&root, &["add", "o.txt"]);
    git(&root, &["commit", "-q", "-m", "O"]);
    let params = json!({
        "source": { "name": "feature" }, "target": { "name": "other" }, "action": "interactiveRebase"
    });
    run(&registry, s, "vcs.workspace.dropAction", params).unwrap();
    let plan = run(&registry, s, "vcs.rebase.state", json!({})).unwrap()["plan"].clone();
    assert_eq!((plan["branch"].clone(), plan["checkout"].clone()), (json!("feature"), json!(true)));
    assert_eq!(step_subjects(&plan), ["B", "C", "D", "F"]);
    run(&registry, s, "vcs.rebase.edit", json!({ "index": 0, "action": "drop" })).unwrap();
    run(&registry, s, "vcs.rebase.start", json!({})).unwrap();
    assert_eq!(git(&root, &["branch", "--show-current"]).trim(), "feature");
    assert_eq!(subjects(&root), ["F", "D", "C", "O", "First"]);
}

#[test]
fn commits_picked_with_a_toggle_are_cherry_picked_in_the_editor() {
    let (_dir, root) = history();
    git(&root, &["switch", "-q", "-c", "side", "HEAD~3"]);
    let (registry, mut session) = super::super::tests::session(&root);
    let s = &mut session;
    run(&registry, s, "vcs.workspace.open", json!({})).unwrap();
    let (b, d) = (rev(&root, "main~2"), rev(&root, "main"));
    run(&registry, s, "vcs.workspace.select", json!({ "revision": d })).unwrap();
    run(&registry, s, "vcs.workspace.select", json!({ "revision": b, "toggle": true })).unwrap();
    let state = run(&registry, s, "vcs.workspace.state", json!({})).unwrap();
    assert_eq!(state["picked"], json!([d, b]), "the selected commit is picked first");
    run(&registry, s, "vcs.workspace.select", json!({ "revision": b, "toggle": true })).unwrap();
    assert_eq!(run(&registry, s, "vcs.workspace.state", json!({})).unwrap()["picked"], json!([d]));
    run(&registry, s, "vcs.workspace.select", json!({ "revision": b, "toggle": true })).unwrap();
    run(&registry, s, "vcs.workspace.menu", json!({ "revision": b })).unwrap();
    let opened =
        run(&registry, s, "vcs.workspace.commitAction", json!({ "revision": b, "action": "cherryPickPicked" }))
            .unwrap();
    assert_eq!(opened["dialog"], "interactiveRebase");
    let plan = run(&registry, s, "vcs.rebase.state", json!({})).unwrap()["plan"].clone();
    assert_eq!(plan["kind"], "cherryPick");
    assert_eq!(step_subjects(&plan), ["B", "D"], "the oldest first");
    assert_eq!(plan["ontoLabel"], "side");
    assert_eq!(run(&registry, s, "vcs.workspace.state", json!({})).unwrap()["picked"], json!([]));
    run(&registry, s, "vcs.rebase.edit", json!({ "index": 0, "action": "drop" })).unwrap();
    run(&registry, s, "vcs.rebase.edit", json!({ "index": 1, "action": "drop" })).unwrap();
    assert!(run(&registry, s, "vcs.rebase.start", json!({})).is_err());
    run(&registry, s, "vcs.rebase.reset", json!({})).unwrap();
    run(&registry, s, "vcs.rebase.start", json!({})).unwrap();
    assert_eq!(subjects(&root), ["D", "B", "First"]);
    assert_eq!(git(&root, &["branch", "--show-current"]).trim(), "side");
    let list = run(&registry, s, "vcs.history.list", json!({})).unwrap();
    assert_eq!(list["undo"]["operation"], "cherryPickMany");
    assert_eq!(list["undo"]["title"], "Cherry-Pick 2 Commits");
    run(&registry, s, "vcs.history.undo", json!({})).unwrap();
    assert_eq!(subjects(&root), ["First"]);

    run(&registry, s, "vcs.workspace.select", json!({ "revision": b, "toggle": true })).unwrap();
    run(&registry, s, "vcs.workspace.select", json!({ "revision": d })).unwrap();
    assert_eq!(run(&registry, s, "vcs.workspace.state", json!({})).unwrap()["picked"], json!([]));
}

fn conflicting() -> (tempfile::TempDir, PathBuf) {
    let dir = test_repository();
    let root = dir.path().canonicalize().unwrap();
    for (file, text, subject) in [("a.txt", "b\n", "B"), ("c.txt", "c\n", "C"), ("a.txt", "d\n", "D")] {
        std::fs::write(root.join(file), text).unwrap();
        git(&root, &["add", file]);
        git(&root, &["commit", "-q", "-m", subject]);
    }
    (dir, root)
}

#[test]
fn conflicts_hand_over_to_the_work_in_progress_until_continue() {
    let (_dir, root) = conflicting();
    let (registry, mut session) = super::super::tests::session(&root);
    let s = &mut session;
    let before = rev(&root, "HEAD");
    run(&registry, s, "vcs.workspace.open", json!({})).unwrap();
    run(&registry, s, "vcs.rebase.plan", json!({ "from": "HEAD~3" })).unwrap();
    run(&registry, s, "vcs.rebase.edit", json!({ "index": 0, "action": "drop" })).unwrap();
    run(&registry, s, "vcs.rebase.edit", json!({ "index": 2, "message": "D, reworded" })).unwrap();
    let error = run(&registry, s, "vcs.rebase.start", json!({})).unwrap_err();
    assert_eq!(error.code.as_deref(), Some(CONFLICTS));
    assert!(error.message.contains("a.txt"), "{}", error.message);
    let workspace = run(&registry, s, "vcs.workspace.state", json!({})).unwrap();
    assert_eq!(workspace["wip"], json!(true), "the work in progress shows the conflicts");
    let state = run(&registry, s, "vcs.rebase.state", json!({})).unwrap();
    assert_eq!(state["open"], json!(false));
    let progress = &state["inProgress"];
    assert_eq!(progress["conflicts"], json!(["a.txt"]));
    assert_eq!((progress["step"].clone(), progress["total"].clone()), (json!(3), json!(3)));
    assert_eq!(progress["stoppedSubject"], "D");
    assert_eq!(progress["branch"], "main");
    assert_eq!(progress["interactive"], json!(true));
    assert!(run(&registry, s, "vcs.history.undo", json!({})).is_err());
    let error = run(&registry, s, "vcs.rebase.continue", json!({})).unwrap_err();
    assert_eq!(error.code.as_deref(), Some(CONFLICTS));
    assert!(run(&registry, s, "vcs.rebase.plan", json!({ "from": "HEAD~1" })).is_err(), "one at a time");

    std::fs::write(root.join("a.txt"), "resolved\n").unwrap();
    run(&registry, s, "vcs.index.stage", json!({ "paths": [root.join("a.txt")] })).unwrap();
    let done = run(&registry, s, "vcs.rebase.continue", json!({})).unwrap();
    assert_eq!(done["head"], rev(&root, "HEAD"));
    assert_eq!(subjects(&root), ["D, reworded", "C", "First"]);
    assert_eq!(run(&registry, s, "vcs.rebase.state", json!({})).unwrap()["inProgress"], json!(null));
    let list = run(&registry, s, "vcs.history.list", json!({})).unwrap();
    assert_eq!(list["undo"]["operation"], "interactiveRebase");
    run(&registry, s, "vcs.history.undo", json!({})).unwrap();
    assert_eq!(rev(&root, "HEAD"), before);
}

#[test]
fn abort_puts_everything_back_and_records_nothing() {
    let (_dir, root) = conflicting();
    let (registry, mut session) = super::super::tests::session(&root);
    let s = &mut session;
    let before = rev(&root, "HEAD");
    run(&registry, s, "vcs.rebase.plan", json!({ "from": "HEAD~3" })).unwrap();
    run(&registry, s, "vcs.rebase.edit", json!({ "index": 0, "action": "drop" })).unwrap();
    assert!(run(&registry, s, "vcs.rebase.start", json!({})).is_err());
    let done = run(&registry, s, "vcs.rebase.abort", json!({})).unwrap();
    assert_eq!(done["head"], before);
    assert_eq!(done["branch"], "main");
    assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "d\n");
    assert_eq!(git(&root, &["status", "--porcelain"]), "");
    assert_eq!(run(&registry, s, "vcs.history.list", json!({})).unwrap()["undo"], json!(null));
    assert!(run(&registry, s, "vcs.rebase.abort", json!({})).is_err(), "nothing in progress");
    assert!(run(&registry, s, "vcs.rebase.continue", json!({})).is_err(), "nothing in progress");
}

#[test]
fn a_plain_rebase_that_stopped_is_continued_and_recorded() {
    let (_dir, root) = conflicting();
    git(&root, &["switch", "-q", "-c", "other", "HEAD~3"]);
    std::fs::write(root.join("a.txt"), "other\n").unwrap();
    git(&root, &["commit", "-q", "-am", "Other"]);
    let (registry, mut session) = super::super::tests::session(&root);
    let s = &mut session;
    let error = run(&registry, s, "vcs.branch.rebase", json!({ "onto": "main" })).unwrap_err();
    assert_eq!(error.code.as_deref(), Some(CONFLICTS));
    let _ = s.state_mut::<GitWorkspace>().take_conflicts();
    std::fs::write(root.join("a.txt"), "both\n").unwrap();
    git(&root, &["add", "a.txt"]);
    run(&registry, s, "vcs.rebase.continue", json!({})).unwrap();
    assert_eq!(subjects(&root), ["Other", "D", "C", "B", "First"]);
    let list = run(&registry, s, "vcs.history.list", json!({})).unwrap();
    assert_eq!(list["undo"]["operation"], "rebase");
    run(&registry, s, "vcs.history.undo", json!({})).unwrap();
    assert_eq!(subjects(&root), ["Other", "First"]);
}

#[test]
fn an_agents_rebase_never_rewrites_a_hidden_file() {
    let (_dir, root) = history();
    std::fs::write(root.join(".env"), "SECRET=1\n").unwrap();
    git(&root, &["add", ".env"]);
    git(&root, &["commit", "-q", "-m", "Secret"]);
    let (registry, mut session) = super::super::tests::session(&root);
    let s = &mut session;
    s.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&[".env".to_owned()]);
    let before = rev(&root, "HEAD");
    run(&registry, s, "vcs.rebase.plan", json!({ "from": rev(&root, "HEAD~2") })).unwrap();
    run(&registry, s, "vcs.rebase.edit", json!({ "index": 1, "moveTo": 0 })).unwrap();
    let refused = zen_core::macros::as_agent(s, |s| run(&registry, s, "vcs.rebase.start", json!({})));
    assert!(refused.unwrap_err().message.contains(".env"));
    assert_eq!(rev(&root, "HEAD"), before);
}

#[test]
fn an_agents_cherry_pick_never_writes_a_hidden_file() {
    let (_dir, root) = history();
    git(&root, &["switch", "-q", "-c", "other"]);
    std::fs::write(root.join(".env"), "SECRET=1\n").unwrap();
    git(&root, &["add", ".env"]);
    git(&root, &["commit", "-q", "-m", "Secret"]);
    let secret = rev(&root, "HEAD");
    git(&root, &["switch", "-q", "main"]);
    let (registry, mut session) = super::super::tests::session(&root);
    let s = &mut session;
    s.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&[".env".to_owned()]);
    run(&registry, s, "vcs.rebase.plan", json!({ "cherryPick": [secret] })).unwrap();
    let refused = zen_core::macros::as_agent(s, |s| run(&registry, s, "vcs.rebase.start", json!({})));
    assert!(refused.unwrap_err().message.contains(".env"));
    assert!(!root.join(".env").exists());
}
