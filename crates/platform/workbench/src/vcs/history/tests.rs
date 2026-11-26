use serde_json::{Value, json};

use super::super::testing::*;
use super::*;

struct Fixture {
    root: PathBuf,
    registry: Registry,
    session: Session,
    _dir: tempfile::TempDir,
    data: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = test_repository();
        zen_i18n::set_language(Some("en"));
        git(dir.path(), &["remote", "add", "origin", "https://example.com/none.git"]);
        let root = dir.path().canonicalize().unwrap();
        let (registry, mut session) = super::super::tests::session(&root);
        let data = tempfile::tempdir().unwrap();
        session.state_mut::<GitHistory>().set_folder(data.path().to_owned());
        Fixture { _dir: dir, data, root, registry, session }
    }

    fn run(&mut self, command: &str, params: Value) -> Result<Value, zen_core::DispatchError> {
        self.registry.dispatch(&mut self.session, command, params)
    }

    fn ok(&mut self, command: &str, params: Value) -> Value {
        self.run(command, params).unwrap_or_else(|error| panic!("{command}: {error:?}"))
    }

    fn refused(&mut self, command: &str) -> (Option<String>, String) {
        match self.run(command, json!({})) {
            Err(zen_core::DispatchError::Failed { error, .. }) => (error.code, error.message),
            other => panic!("{command} was not refused: {other:?}"),
        }
    }

    fn git(&self, args: &[&str]) -> String {
        git(&self.root, args).trim().to_owned()
    }

    fn rev(&self, revision: &str) -> String {
        self.git(&["rev-parse", revision])
    }

    fn branch(&self) -> String {
        self.git(&["branch", "--show-current"])
    }

    fn write(&self, file: &str, text: &str) {
        std::fs::write(self.root.join(file), text).unwrap();
    }

    fn read(&self, file: &str) -> Option<String> {
        std::fs::read_to_string(self.root.join(file)).ok()
    }

    fn list(&mut self) -> Value {
        self.ok("vcs.history.list", json!({}))
    }

    fn undo_title(&mut self) -> Value {
        self.list()["undo"]["title"].clone()
    }

    fn commit_in_terminal(&self, file: &str, text: &str, message: &str) {
        self.write(file, text);
        self.git(&["add", file]);
        self.git(&["commit", "-q", "-m", message]);
    }
}

#[test]
fn a_commit_is_undone_to_staged_changes_and_redone() {
    let mut f = Fixture::new();
    let first = f.rev("HEAD");
    f.write("a.txt", "one\nTWO\nthree\n");
    let made = f.ok("vcs.commit.create", json!({ "message": "Fix login\n\nWhy.", "paths": ["a.txt"] }));
    assert_eq!(f.undo_title(), "Commit 'Fix login'");
    let list = f.list();
    assert_eq!(list["undo"]["operation"], "commit");
    assert_eq!(list["undo"]["before"]["head"], first);
    assert_eq!(list["undo"]["after"]["head"], made["commit"]);
    assert_eq!(list["undo"]["branches"], json!(["main"]));
    assert_eq!(list["undo"]["reflog"], json!(["commit: Fix login"]), "from git's reflog");
    assert_eq!(list["redo"], json!(null));

    let undone = f.ok("vcs.history.undo", json!({}));
    assert_eq!(undone["entry"]["title"], "Commit 'Fix login'");
    assert_eq!(undone["head"], json!({ "head": first, "branch": "main" }));
    assert_eq!(f.rev("HEAD"), first);
    assert_eq!(f.git(&["diff", "--cached", "--name-only"]), "a.txt", "the changes are staged again");
    assert_eq!(f.read("a.txt").unwrap(), "one\nTWO\nthree\n");
    let list = f.list();
    assert_eq!(list["undo"], json!(null));
    assert_eq!(list["redo"]["title"], "Commit 'Fix login'");
    assert!(f.git(&["reflog", "-1", "--format=%gs"]).starts_with("zen undo commit: Fix login"));

    let redone = f.ok("vcs.history.redo", json!({}));
    assert_eq!(redone["head"]["head"], made["commit"]);
    assert_eq!(f.rev("HEAD"), made["commit"].as_str().unwrap());
    assert_eq!(f.git(&["status", "--porcelain"]), "", "nothing left staged");
    assert_eq!(f.refused("vcs.history.redo").0, None, "nothing to redo");
    assert!(f.refused("vcs.history.redo").1.contains("redo"));
}

#[test]
fn an_amend_is_undone_to_the_old_commit() {
    let mut f = Fixture::new();
    let first = f.rev("HEAD");
    f.ok("vcs.commit.create", json!({ "message": "First, better", "amend": true }));
    assert_eq!(f.undo_title(), "Amend Commit 'First, better'");
    assert_ne!(f.rev("HEAD"), first);
    f.ok("vcs.history.undo", json!({}));
    assert_eq!(f.rev("HEAD"), first);
    assert_eq!(f.git(&["log", "-1", "--format=%s"]), "First");
    f.ok("vcs.history.redo", json!({}));
    assert_eq!(f.git(&["log", "-1", "--format=%s"]), "First, better");
}

#[test]
fn a_commit_of_what_is_staged_is_recorded_too() {
    let mut f = Fixture::new();
    f.write("a.txt", "staged\n");
    f.git(&["add", "a.txt"]);
    f.ok("vcs.commit.create", json!({ "message": "Staged", "staged": true }));
    assert_eq!(f.undo_title(), "Commit 'Staged'");
}

#[test]
fn checkouts_of_branches_and_revisions_are_undone_and_redone() {
    let mut f = Fixture::new();
    f.git(&["branch", "feature"]);
    f.ok("vcs.branch.checkout", json!({ "name": "feature" }));
    assert_eq!(f.undo_title(), "Checkout 'feature'");
    f.ok("vcs.history.undo", json!({}));
    assert_eq!(f.branch(), "main");
    f.ok("vcs.history.redo", json!({}));
    assert_eq!(f.branch(), "feature");

    let first = f.rev("HEAD");
    f.ok("vcs.revision.checkout", json!({ "revision": first }));
    assert_eq!(f.branch(), "");
    f.ok("vcs.history.undo", json!({}));
    assert_eq!(f.branch(), "feature");
    f.ok("vcs.history.redo", json!({}));
    assert_eq!(f.branch(), "", "detached again");
    assert_eq!(f.rev("HEAD"), first);

    f.ok("vcs.branch.checkout", json!({ "name": "main" }));
    let before = f.list()["done"].as_array().unwrap().len();
    f.ok("vcs.branch.checkout", json!({ "name": "main" }));
    assert_eq!(f.list()["done"].as_array().unwrap().len(), before);
}

#[test]
fn a_checkout_of_a_remote_branch_is_undone_with_the_branch_it_made() {
    let mut f = Fixture::new();
    let first = f.rev("HEAD");
    f.git(&["update-ref", "refs/remotes/origin/topic", first.as_str()]);
    f.ok("vcs.branch.checkout", json!({ "name": "origin/topic" }));
    assert_eq!(f.branch(), "topic");
    f.ok("vcs.history.undo", json!({}));
    assert_eq!(f.branch(), "main");
    assert_eq!(f.git(&["branch", "--list", "topic"]), "", "the branch it made is gone");
    f.ok("vcs.history.redo", json!({}));
    assert_eq!(f.branch(), "topic");
    assert_eq!(f.git(&["rev-parse", "--abbrev-ref", "topic@{upstream}"]), "origin/topic", "tracking again");
}

#[test]
fn branches_created_and_deleted_come_back_and_go_again() {
    let mut f = Fixture::new();
    let first = f.rev("HEAD");
    f.ok("vcs.branch.create", json!({ "name": "feature" }));
    assert_eq!(f.undo_title(), "Create Branch 'feature'");
    assert_eq!(f.branch(), "feature");
    f.ok("vcs.history.undo", json!({}));
    assert_eq!(f.branch(), "main");
    assert_eq!(f.git(&["branch", "--list", "feature"]), "");
    f.ok("vcs.history.redo", json!({}));
    assert_eq!(f.branch(), "feature");
    f.ok("vcs.branch.checkout", json!({ "name": "main" }));

    f.git(&["switch", "-q", "feature"]);
    f.commit_in_terminal("f.txt", "feature\n", "Feature");
    let feature = f.rev("feature");
    f.git(&["update-ref", "refs/remotes/origin/feature", first.as_str()]);
    f.git(&["branch", "-q", "--set-upstream-to", "origin/feature", "feature"]);
    f.git(&["switch", "-q", "main"]);
    f.ok("vcs.branch.delete", json!({ "name": "feature", "force": true }));
    assert_eq!(f.undo_title(), "Delete Branch 'feature'");
    f.ok("vcs.history.undo", json!({}));
    assert_eq!(f.rev("feature"), feature, "back at its commit");
    assert_eq!(f.git(&["rev-parse", "--abbrev-ref", "feature@{upstream}"]), "origin/feature", "and its upstream");
    assert_eq!(f.branch(), "main");
    f.ok("vcs.history.redo", json!({}));
    assert_eq!(f.git(&["branch", "--list", "feature"]), "");
}

#[test]
fn resets_are_undone_as_their_mode_needs() {
    let mut f = Fixture::new();
    f.commit_in_terminal("a.txt", "second\n", "Second");
    let (first, second) = (f.rev("HEAD~1"), f.rev("HEAD"));

    f.ok("vcs.branch.reset", json!({ "revision": first, "mode": "soft" }));
    assert_eq!(f.undo_title(), format!("Reset 'main' to {}", &first[..7]));
    f.ok("vcs.history.undo", json!({}));
    assert_eq!(f.rev("HEAD"), second);
    assert_eq!(f.git(&["status", "--porcelain"]), "", "soft: nothing staged afterwards");

    f.ok("vcs.branch.reset", json!({ "revision": first, "mode": "mixed" }));
    f.ok("vcs.history.undo", json!({}));
    assert_eq!(f.rev("HEAD"), second);
    assert_eq!(f.git(&["status", "--porcelain"]), "", "mixed: clean again");
    f.ok("vcs.history.redo", json!({}));
    assert_eq!(f.rev("HEAD"), first);
    assert_eq!(f.git(&["diff", "--name-only"]), "a.txt", "mixed again: unstaged");
    f.ok("vcs.history.undo", json!({}));

    f.write("a.txt", "second, edited\n");
    f.write("new.txt", "staged new file\n");
    f.git(&["add", "new.txt"]);
    f.write("untracked.txt", "left alone\n");
    f.ok("vcs.branch.reset", json!({ "revision": first, "mode": "hard" }));
    assert_eq!(f.read("a.txt").unwrap(), "one\ntwo\nthree\n");
    assert_eq!(f.read("new.txt"), None);
    let list = f.list();
    assert_eq!(list["undo"]["files"], json!(["a.txt", "new.txt"]), "the changes thrown away are kept");
    f.ok("vcs.history.undo", json!({}));
    assert_eq!(f.rev("HEAD"), second);
    assert_eq!(f.read("a.txt").unwrap(), "second, edited\n");
    assert_eq!(f.read("new.txt").unwrap(), "staged new file\n");
    assert_eq!(f.git(&["diff", "--cached", "--name-only"]), "new.txt", "staged as it was");
    assert_eq!(f.git(&["diff", "--name-only"]), "a.txt", "unstaged as it was");
    assert_eq!(f.read("untracked.txt").unwrap(), "left alone\n");
    f.ok("vcs.history.redo", json!({}));
    assert_eq!(f.rev("HEAD"), first);
    assert_eq!(f.read("a.txt").unwrap(), "one\ntwo\nthree\n");
    assert_eq!(f.read("new.txt"), None);
}

fn diverged(f: &Fixture) {
    f.git(&["switch", "-q", "-c", "feature"]);
    f.commit_in_terminal("f.txt", "feature\n", "Feature");
    f.git(&["switch", "-q", "main"]);
    f.commit_in_terminal("a.txt", "second\n", "Second");
}

#[test]
fn merges_and_fast_forwards_are_undone_and_redone() {
    let mut f = Fixture::new();
    diverged(&f);
    let main = f.rev("main");
    f.ok("vcs.branch.merge", json!({ "name": "feature" }));
    assert_eq!(f.undo_title(), "Merge 'feature' into 'main'");
    let merged = f.rev("main");
    f.ok("vcs.history.undo", json!({}));
    assert_eq!(f.rev("main"), main);
    assert_eq!(f.read("f.txt"), None, "the files follow");
    f.ok("vcs.history.redo", json!({}));
    assert_eq!(f.rev("main"), merged);

    f.git(&["branch", "behind", "main~1"]);
    let behind = f.rev("behind");
    f.ok("vcs.branch.merge", json!({ "source": "main", "target": "behind" }));
    assert_eq!(f.branch(), "behind");
    f.ok("vcs.history.undo", json!({}));
    assert_eq!((f.branch().as_str(), f.rev("behind")), ("main", behind.clone()));

    f.ok("vcs.branch.fastForward", json!({ "name": "behind", "onto": "main" }));
    assert_eq!(f.undo_title(), "Fast-forward 'behind'");
    f.ok("vcs.history.undo", json!({}));
    assert_eq!(f.rev("behind"), behind);
}

#[test]
fn rebases_cherry_picks_and_reverts_are_undone_and_redone() {
    let mut f = Fixture::new();
    diverged(&f);
    let feature = f.rev("feature");
    f.ok("vcs.branch.rebase", json!({ "onto": "main", "branch": "feature" }));
    assert_eq!(f.undo_title(), "Rebase 'feature' onto 'main'");
    let rebased = f.rev("feature");
    f.ok("vcs.history.undo", json!({}));
    assert_eq!((f.branch().as_str(), f.rev("feature")), ("main", feature.clone()), "back on main, feature as it was");
    f.ok("vcs.history.redo", json!({}));
    assert_eq!((f.branch().as_str(), f.rev("feature")), ("feature", rebased));
    f.git(&["switch", "-q", "main"]);

    let main = f.rev("main");
    f.ok("vcs.commit.cherryPick", json!({ "id": feature }));
    assert_eq!(f.undo_title(), "Cherry-Pick 'Feature'");
    f.ok("vcs.history.undo", json!({}));
    assert_eq!(f.rev("main"), main);
    assert_eq!(f.read("f.txt"), None);
    f.ok("vcs.history.redo", json!({}));
    assert_eq!(f.read("f.txt").unwrap(), "feature\n");

    f.ok("vcs.commit.revert", json!({ "id": "HEAD" }));
    assert_eq!(f.undo_title(), "Revert 'Feature'");
    f.ok("vcs.history.undo", json!({}));
    assert_eq!(f.read("f.txt").unwrap(), "feature\n");
}

#[test]
fn an_operation_that_stops_on_conflicts_is_not_recorded() {
    let mut f = Fixture::new();
    f.git(&["switch", "-q", "-c", "other"]);
    f.commit_in_terminal("a.txt", "other\n", "Other");
    f.git(&["switch", "-q", "main"]);
    f.commit_in_terminal("a.txt", "main\n", "Main");
    assert!(f.run("vcs.branch.merge", json!({ "name": "other" })).is_err());
    assert_eq!(f.list()["undo"], json!(null));
    f.git(&["merge", "--abort"]);
    f.ok("vcs.branch.create", json!({ "name": "x", "checkout": false }));
    assert!(f.run("vcs.branch.merge", json!({ "name": "other" })).is_err());
    let (code, message) = f.refused("vcs.history.undo");
    assert_eq!(code.as_deref(), Some(MOVED));
    assert!(message.contains("merge is in progress"), "{message}");
    assert!(f.git(&["branch", "--list", "x"]).contains('x'), "nothing changed");
}

#[test]
fn discarded_changes_and_deleted_unversioned_files_come_back() {
    let mut f = Fixture::new();
    f.write("a.txt", "one\nedited\nthree\n");
    f.write("b.txt", "unversioned\n");
    f.ok("vcs.changes.discard", json!({ "paths": ["a.txt", "b.txt"] }));
    assert_eq!(f.undo_title(), "Discard Changes in 2 Files");
    assert_eq!(f.read("a.txt").unwrap(), "one\ntwo\nthree\n");
    assert_eq!(f.read("b.txt"), None);
    let kept = std::fs::read_dir(f.data.path()).unwrap().flatten().next().unwrap().path().join("files");
    assert!(std::fs::read_dir(&kept).unwrap().count() >= 2);
    f.ok("vcs.history.undo", json!({}));
    assert_eq!(f.read("a.txt").unwrap(), "one\nedited\nthree\n");
    assert_eq!(f.read("b.txt").unwrap(), "unversioned\n");
    f.ok("vcs.history.redo", json!({}));
    assert_eq!(f.read("a.txt").unwrap(), "one\ntwo\nthree\n");
    assert_eq!(f.read("b.txt"), None);
    f.ok("vcs.history.undo", json!({}));

    f.write("a.txt", "one\nedited\nthree\nfour\n");
    f.ok("vcs.changes.discard", json!({ "paths": ["a.txt"], "lines": [3] }));
    assert_eq!(f.undo_title(), "Discard Changes in 'a.txt'");
    assert_eq!(f.read("a.txt").unwrap(), "one\nedited\nthree\n");
    f.ok("vcs.history.undo", json!({}));
    assert_eq!(f.read("a.txt").unwrap(), "one\nedited\nthree\nfour\n");

    f.ok("vcs.changes.discard", json!({ "all": true }));
    assert_eq!(f.read("a.txt").unwrap(), "one\ntwo\nthree\n");
    f.ok("vcs.history.undo", json!({}));
    assert_eq!(f.read("a.txt").unwrap(), "one\nedited\nthree\nfour\n");
}

#[test]
fn a_rollback_brings_back_staged_and_unstaged_changes() {
    let mut f = Fixture::new();
    f.write("a.txt", "staged\n");
    f.git(&["add", "a.txt"]);
    f.write("a.txt", "staged and more\n");
    f.write("n.txt", "new\n");
    f.git(&["add", "n.txt"]);
    f.ok("vcs.changes.rollback", json!({ "paths": ["a.txt", "n.txt"], "deleteAdded": true }));
    assert_eq!(f.undo_title(), "Rollback 2 Files");
    assert_eq!(f.git(&["status", "--porcelain"]), "");
    f.ok("vcs.history.undo", json!({}));
    assert_eq!(f.read("a.txt").unwrap(), "staged and more\n");
    assert_eq!(f.git(&["show", ":a.txt"]), "staged");
    assert_eq!(f.read("n.txt").unwrap(), "new\n");
    assert_eq!(f.git(&["diff", "--cached", "--name-only"]), "a.txt\nn.txt");
}

#[test]
fn undo_is_refused_when_the_repository_moved_since() {
    let mut f = Fixture::new();
    f.write("a.txt", "mine\n");
    f.ok("vcs.commit.create", json!({ "message": "Mine", "paths": ["a.txt"] }));
    f.commit_in_terminal("a.txt", "theirs\n", "Theirs");
    let head = f.rev("HEAD");
    let (code, message) = f.refused("vcs.history.undo");
    assert_eq!(code.as_deref(), Some(MOVED));
    assert!(message.starts_with("Cannot undo Commit 'Mine': main is at"), "{message}");
    assert_eq!(f.rev("HEAD"), head, "nothing changed");
    assert_eq!(f.list()["undo"]["title"], "Commit 'Mine'", "still there to undo");

    f.ok("vcs.branch.create", json!({ "name": "side", "checkout": false }));
    f.git(&["switch", "-q", "side"]);
    let (code, message) = f.refused("vcs.history.undo");
    assert_eq!(code.as_deref(), Some(MOVED));
    assert!(message.contains("HEAD is on side now, not on main"), "{message}");

    f.git(&["switch", "-q", "main"]);
    f.write("a.txt", "edited\n");
    f.ok("vcs.changes.discard", json!({ "paths": ["a.txt"] }));
    f.write("a.txt", "edited again\n");
    let (code, message) = f.refused("vcs.history.undo");
    assert_eq!(code.as_deref(), Some(MOVED));
    assert!(message.contains("a.txt was changed since"), "{message}");
    assert_eq!(f.read("a.txt").unwrap(), "edited again\n", "the new edit stays");
}

#[test]
fn undo_never_overwrites_uncommitted_changes() {
    let mut f = Fixture::new();
    diverged(&f);
    f.ok("vcs.branch.merge", json!({ "name": "feature" }));
    let merged = f.rev("HEAD");
    f.write("f.txt", "edited\n");
    match f.run("vcs.history.undo", json!({})) {
        Err(zen_core::DispatchError::Failed { error, .. }) => {
            assert!(error.message.starts_with("Undoing Merge 'feature' into 'main' failed"), "{}", error.message)
        }
        other => panic!("not refused: {other:?}"),
    }
    assert_eq!(f.rev("HEAD"), merged);
    assert_eq!(f.read("f.txt").unwrap(), "edited\n");
    assert_eq!(f.undo_title(), "Merge 'feature' into 'main'", "still there to undo");
}

#[test]
fn a_new_operation_forgets_what_could_be_redone_and_ids_guard_the_step() {
    let mut f = Fixture::new();
    f.ok("vcs.branch.create", json!({ "name": "one", "checkout": false }));
    f.ok("vcs.branch.create", json!({ "name": "two", "checkout": false }));
    let list = f.list();
    let (two, one) = (list["done"][0]["id"].as_u64().unwrap(), list["done"][1]["id"].as_u64().unwrap());
    assert!(f.run("vcs.history.undo", json!({ "id": one })).is_err(), "two is next");
    f.ok("vcs.history.undo", json!({ "id": two }));
    assert_eq!(f.list()["redo"]["id"], two);
    f.ok("vcs.branch.create", json!({ "name": "three", "checkout": false }));
    assert_eq!(f.list()["redo"], json!(null));
    assert_eq!(f.list()["undone"], json!([]));
}

#[test]
fn the_journal_is_kept_in_the_data_folder() {
    let mut f = Fixture::new();
    f.ok("vcs.branch.create", json!({ "name": "kept", "checkout": false }));
    let (registry, mut session) = super::super::tests::session(&f.root);
    session.state_mut::<GitHistory>().set_folder(f.data.path().to_owned());
    let list = registry.dispatch(&mut session, "vcs.history.list", json!({})).unwrap();
    assert_eq!(list["undo"]["title"], "Create Branch 'kept'");
    registry.dispatch(&mut session, "vcs.history.undo", json!({})).unwrap();
    assert_eq!(f.list()["redo"]["title"], "Create Branch 'kept'");
}

#[test]
fn operations_of_the_git_workspace_are_recorded() {
    let mut f = Fixture::new();
    diverged(&f);
    f.ok("vcs.workspace.open", json!({}));
    let local = |name: &str| json!({ "name": name, "kind": "local" });
    f.ok("vcs.workspace.dropMenu", json!({ "source": local("feature"), "target": local("main") }));
    let action = json!({ "source": local("feature"), "target": local("main"), "action": "merge", "confirmed": false });
    f.ok("vcs.workspace.dropAction", action);
    assert_eq!(f.undo_title(), "Merge 'feature' into 'main'");
}

#[test]
fn moved_names_what_changed() {
    isolate();
    zen_i18n::set_language(Some("en"));
    let refs = |branch: Option<&str>, head: &str, branches: &[(&str, &str)]| Refs {
        branch: branch.map(str::to_owned),
        head: Some(head.to_owned()),
        branches: branches.iter().map(|(name, id)| ((*name).to_owned(), (*id).to_owned())).collect(),
        upstreams: BTreeMap::new(),
    };
    let after = refs(Some("main"), "2222222aaa", &[("main", "2222222aaa"), ("other", "9999999aaa")]);
    let before = refs(Some("main"), "1111111aaa", &[("main", "1111111aaa"), ("other", "9999999aaa")]);
    assert_eq!(moved(&after, &before, &after), None);
    let other_moved = refs(Some("main"), "2222222aaa", &[("main", "2222222aaa"), ("other", "8888888aaa")]);
    assert_eq!(moved(&after, &before, &other_moved), None);
    let main_moved = refs(Some("main"), "3333333aaa", &[("main", "3333333aaa")]);
    assert_eq!(
        moved(&after, &before, &main_moved).unwrap(),
        "main is at 3333333 now, not at 2222222. The repository changed since."
    );
    let elsewhere = refs(None, "2222222aaa", &[("main", "2222222aaa")]);
    assert!(moved(&after, &before, &elsewhere).unwrap().starts_with("HEAD is on 2222222 now, not on main."));
    let created = refs(Some("main"), "1111111aaa", &[("main", "1111111aaa")]);
    let with_new = refs(Some("main"), "1111111aaa", &[("main", "1111111aaa"), ("new", "1111111aaa")]);
    assert_eq!(moved(&created, &with_new, &with_new).unwrap(), "there is a branch new now.");
    assert_eq!(moved(&with_new, &created, &created).unwrap(), "new was deleted since.");
}

#[test]
fn repositories_have_stable_folders() {
    assert_eq!(key(Path::new("/work/my repo")), key(Path::new("/work/my repo")));
    assert_ne!(key(Path::new("/work/a")), key(Path::new("/other/a")));
    assert!(key(Path::new("/work/my repo")).starts_with("my_repo-"));
}
