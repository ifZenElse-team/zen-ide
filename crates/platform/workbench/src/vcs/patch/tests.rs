use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};

use super::super::changelists::LocalChanges;
use super::super::testing::*;
use super::*;
use crate::file_clipboard::{MemoryClipboard, SystemClipboard};

fn session(root: &Path, data: &Path) -> (Registry, Session, Arc<MemoryClipboard>) {
    zen_i18n::set_language(Some("en"));
    let (registry, mut session) = super::super::tests::session(root);
    session.state_mut::<LocalChanges>().set_folder(data.to_owned());
    session.state_mut::<history::GitHistory>().set_folder(data.join("vcs-history"));
    let clipboard = Arc::new(MemoryClipboard::default());
    crate::file_clipboard::install(&mut session, clipboard.clone());
    (registry, session, clipboard)
}

fn run(registry: &Registry, session: &mut Session, id: &str, params: Value) -> Value {
    registry.dispatch(session, id, params.clone()).unwrap_or_else(|error| panic!("{id} {params}: {error:?}"))
}

fn fails(registry: &Registry, session: &mut Session, id: &str, params: Value) -> String {
    match registry.dispatch(session, id, params) {
        Err(zen_core::DispatchError::Failed { error, .. }) => error.message,
        other => panic!("{id} should fail: {other:?}"),
    }
}

fn clone(root: &Path) -> tempfile::TempDir {
    let other = tempfile::tempdir().unwrap();
    git(other.path(), &["clone", "-q", root.to_str().unwrap(), "."]);
    other
}

#[test]
fn local_changes_become_a_git_patch_that_git_applies_and_ams() {
    let dir = test_repository();
    let root = dir.path().canonicalize().unwrap();
    let data = tempfile::tempdir().unwrap();
    let elsewhere = clone(&root);
    let (registry, mut session, clipboard) = session(&root, data.path());
    let s = &mut session;
    std::fs::write(root.join("a.txt"), "one\nTWO\nthree\n").unwrap();
    std::fs::create_dir(root.join("sub")).unwrap();
    std::fs::write(root.join("sub/new file.txt"), "new\n").unwrap();
    run(&registry, s, "file.document.open", json!({ "path": root.join("a.txt") }));
    s.state_mut::<Workbench>().active_document_mut().unwrap().replace_ranges(vec![(14..14, "four\n".to_owned())]);
    run(&registry, s, "vcs.status.refresh", json!({ "wait": true }));
    let out = data.path().join("out/changes.patch");
    let made = run(
        &registry,
        s,
        "vcs.patch.create",
        json!({ "paths": ["a.txt", "sub/new file.txt"], "output": out, "message": "Local work" }),
    );
    assert_eq!(made["files"], json!(["a.txt", "sub/new file.txt"]), "{made}");
    assert_eq!(made["text"], Value::Null);
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.starts_with("From 0000000000000000000000000000000000000000 Mon Sep 17 00:00:00 2001\n"), "{text}");
    assert!(text.contains("From: Ada Lovelace <ada@example.com>\n"), "{text}");
    assert!(text.contains("Subject: [PATCH] Local work\n"), "{text}");
    assert!(text.contains("diff --git a/a.txt b/a.txt\n"), "{text}");
    assert!(text.contains("+four\n"), "unsaved edits are in the patch: {text}");
    assert!(text.contains("new file mode 100644"), "{text}");
    git(elsewhere.path(), &["apply", "--check", out.to_str().unwrap()]);
    git(elsewhere.path(), &["am", "-q", out.to_str().unwrap()]);
    assert_eq!(std::fs::read_to_string(elsewhere.path().join("a.txt")).unwrap(), "one\nTWO\nthree\nfour\n");
    assert_eq!(
        git(elsewhere.path(), &["log", "-1", "--format=%an <%ae>|%s"]).trim(),
        "Ada Lovelace <ada@example.com>|Local work"
    );

    let copied = run(&registry, s, "vcs.patch.create", json!({ "clipboard": true }));
    assert_eq!(copied["clipboard"], true);
    assert_eq!(copied["files"], json!(["a.txt"]), "the active changelist's files: {copied}");
    assert_eq!(clipboard.text(), copied["text"].as_str().unwrap());
    assert!(clipboard.text().contains("Subject: [PATCH] Changes\n"));

    let reversed = run(&registry, s, "vcs.patch.create", json!({ "paths": ["a.txt"], "reverse": true }));
    let reversed_file = data.path().join("reversed.patch");
    std::fs::write(&reversed_file, reversed["text"].as_str().unwrap()).unwrap();
    git(&root, &["apply", "--check", reversed_file.to_str().unwrap()]);

    let inside = run(&registry, s, "vcs.patch.create", json!({ "paths": ["sub/new file.txt"], "basePath": "sub" }));
    assert_eq!(inside["files"], json!(["new file.txt"]));
    assert!(inside["text"].as_str().unwrap().contains("diff --git a/new file.txt b/new file.txt"));
    let outside = fails(&registry, s, "vcs.patch.create", json!({ "paths": ["a.txt"], "basePath": "sub" }));
    assert!(outside.contains("outside the base path"), "{outside}");
    let above =
        run(&registry, s, "vcs.patch.create", json!({ "paths": ["a.txt"], "basePath": root.parent().unwrap() }));
    let name = root.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(above["files"], json!([format!("{name}/a.txt")]));
    let nowhere = fails(&registry, s, "vcs.patch.create", json!({ "paths": ["a.txt"], "basePath": "/nowhere/else" }));
    assert!(nowhere.contains("neither inside the repository nor above it"), "{nowhere}");
    assert!(
        fails(&registry, s, "vcs.patch.create", json!({ "paths": ["a.txt"], "encoding": "klingon" }))
            .contains("klingon")
    );
}

#[test]
fn a_commit_becomes_a_patch_that_git_ams_with_its_author_and_message() {
    let dir = test_repository();
    let root = dir.path().canonicalize().unwrap();
    let data = tempfile::tempdir().unwrap();
    let elsewhere = clone(&root);
    std::fs::write(root.join("a.txt"), "one\ntwo\nthree\nfour\n").unwrap();
    std::fs::write(root.join("logo.bin"), [0u8, 159, 146, 150, 0, 1]).unwrap();
    git(&root, &["add", "-A"]);
    git(
        &root,
        &[
            "-c",
            "user.name=Grace Hopper",
            "-c",
            "user.email=grace@example.com",
            "commit",
            "-q",
            "--author=Grace Hopper <grace@example.com>",
            "-m",
            "Add four",
            "-m",
            "Because four comes after three.",
        ],
    );
    let (registry, mut session, _) = session(&root, data.path());
    let s = &mut session;
    let out = data.path().join("commit.patch");
    run(&registry, s, "vcs.patch.create", json!({ "source": "commit", "revision": "HEAD", "output": out }));
    let text = std::fs::read_to_string(&out).unwrap();
    let id = git(&root, &["rev-parse", "HEAD"]);
    assert!(
        text.starts_with(&format!(
            "From {} Mon Sep 17 00:00:00 2001\nFrom: Grace Hopper <grace@example.com>\n",
            id.trim()
        )),
        "{text}"
    );
    assert!(text.contains("GIT binary patch"), "{text}");
    git(elsewhere.path(), &["apply", "--check", out.to_str().unwrap()]);
    git(elsewhere.path(), &["am", "-q", out.to_str().unwrap()]);
    let log = git(elsewhere.path(), &["log", "-1", "--format=%an <%ae>|%s|%b"]);
    assert_eq!(log.trim(), "Grace Hopper <grace@example.com>|Add four|Because four comes after three.");
    assert_eq!(std::fs::read(elsewhere.path().join("logo.bin")).unwrap(), [0u8, 159, 146, 150, 0, 1]);
    assert!(fails(&registry, s, "vcs.patch.create", json!({ "source": "commit" })).contains("commit"));
}

#[test]
fn shelved_changes_become_a_patch() {
    let dir = test_repository();
    let root = dir.path().canonicalize().unwrap();
    let data = tempfile::tempdir().unwrap();
    let (registry, mut session, _) = session(&root, data.path());
    let s = &mut session;
    std::fs::write(root.join("a.txt"), "one\nshelved\nthree\n").unwrap();
    std::fs::write(root.join("b.txt"), "b\n").unwrap();
    run(&registry, s, "vcs.status.refresh", json!({ "wait": true }));
    run(&registry, s, "vcs.shelf.create", json!({ "paths": ["a.txt", "b.txt"], "name": "Later" }));
    let made = run(&registry, s, "vcs.patch.create", json!({ "source": "shelf", "shelf": 1, "paths": ["a.txt"] }));
    assert_eq!(made["files"], json!(["a.txt"]));
    let text = made["text"].as_str().unwrap();
    assert!(text.contains("Subject: [PATCH] Later\n") && text.contains("+shelved\n"), "{text}");
    let file = data.path().join("shelf.patch");
    std::fs::write(&file, text).unwrap();
    git(&root, &["apply", "--check", file.to_str().unwrap()]);
    assert!(fails(&registry, s, "vcs.patch.create", json!({ "source": "shelf", "shelf": 9 })).contains("9"));
}

const FORMAT_PATCH: &str = "From 1111111111111111111111111111111111111111 Mon Sep 17 00:00:00 2001
From: Grace Hopper <grace@example.com>
Date: Thu, 22 Oct 2026 06:05:01 +0800
Subject: [PATCH] Shout two

Louder is better.
---
diff --git a/a.txt b/a.txt
--- a/a.txt
+++ b/a.txt
@@ -1,3 +1,3 @@
 one
-two
+TWO
 three
diff --git a/docs/new.md b/docs/new.md
new file mode 100644
--- /dev/null
+++ b/docs/new.md
@@ -0,0 +1 @@
+# New
";

#[test]
fn a_git_patch_applies_into_a_new_changelist_and_fills_the_commit_in() {
    let dir = test_repository();
    let root = dir.path().canonicalize().unwrap();
    let data = tempfile::tempdir().unwrap();
    let (registry, mut session, clipboard) = session(&root, data.path());
    let s = &mut session;
    run(&registry, s, "file.document.open", json!({ "path": root.join("a.txt") }));
    clipboard.set_text(FORMAT_PATCH);
    let read = run(&registry, s, "vcs.patch.read", json!({ "clipboard": true }));
    assert_eq!(read["source"], "clipboard");
    assert_eq!(read["strip"], 1);
    assert_eq!(read["author"], "Grace Hopper <grace@example.com>");
    assert_eq!(read["message"], "Shout two\n\nLouder is better.");
    assert_eq!(read["files"][0]["relative"], "a.txt");
    assert_eq!(read["files"][0]["state"], "clean");
    assert_eq!(read["files"][1]["status"], "added");
    assert_eq!(read["files"][1]["patchPath"], "b/docs/new.md");
    let diff = run(&registry, s, "vcs.patch.diff", json!({ "clipboard": true, "file": "a.txt" }));
    assert_eq!(
        (diff["before"].as_str(), diff["after"].as_str()),
        (Some("one\ntwo\nthree\n"), Some("one\nTWO\nthree\n"))
    );
    assert_eq!(diff["conflicts"], json!([]));

    let applied = run(&registry, s, "vcs.patch.apply", json!({ "clipboard": true, "changelist": "From Grace" }));
    assert_eq!(applied["applied"], json!(["a.txt", "docs/new.md"]), "{applied}");
    assert_eq!(applied["changelist"], "From Grace");
    assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "one\nTWO\nthree\n");
    assert_eq!(std::fs::read_to_string(root.join("docs/new.md")).unwrap(), "# New\n");
    let text = s.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
    assert_eq!(text, "one\nTWO\nthree\n", "the open file follows");
    let lists = run(&registry, s, "vcs.changelist.list", json!({}));
    let list = lists["lists"].as_array().unwrap().iter().find(|list| list["name"] == "From Grace").unwrap().clone();
    assert_eq!(list["files"][0]["relative"], "a.txt", "{lists}");
    let draft = run(&registry, s, "vcs.commit.draft", json!({}));
    assert_eq!(draft["message"], "Shout two\n\nLouder is better.");
    assert_eq!(draft["author"], "Grace Hopper <grace@example.com>");
    assert_eq!(draft["generation"], 1);
    assert_eq!(run(&registry, s, "vcs.history.list", json!({}))["undo"]["title"], "Apply Patch 'Shout two'");

    run(
        &registry,
        s,
        "vcs.commit.create",
        json!({ "message": draft["message"], "author": draft["author"], "paths": ["a.txt", "docs/new.md"] }),
    );
    let log = git(&root, &["log", "-1", "--format=%an <%ae>|%cn|%s"]);
    assert_eq!(log.trim(), "Grace Hopper <grace@example.com>|Ada Lovelace|Shout two");

    let reverse = run(&registry, s, "vcs.patch.apply", json!({ "text": FORMAT_PATCH, "reverse": true }));
    assert_eq!(reverse["applied"], json!(["a.txt", "docs/new.md"]), "{reverse}");
    assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "one\ntwo\nthree\n");
    assert!(!root.join("docs/new.md").exists(), "a reversed added file is deleted");
    run(&registry, s, "vcs.history.undo", json!({}));
    assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "one\nTWO\nthree\n", "undo takes it back");
}

#[test]
fn paths_map_onto_a_base_folder_with_leading_folders_stripped() {
    let dir = test_repository();
    let root = dir.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join("module/src")).unwrap();
    std::fs::write(root.join("module/src/lib.go"), "package lib\n\nfunc A() {}\n").unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "Module"]);
    let data = tempfile::tempdir().unwrap();
    let (registry, mut session, _) = session(&root, data.path());
    let s = &mut session;
    let text =
        "--- old/project/src/lib.go\n+++ new/project/src/lib.go\n@@ -3 +3 @@\n-func A() {}\n+func A() { println() }\n";
    let file = data.path().join("plain.diff");
    std::fs::write(&file, text).unwrap();
    let missing = run(&registry, s, "vcs.patch.read", json!({ "path": file }));
    assert_eq!(missing["strip"], 0);
    assert_eq!(missing["files"][0]["state"], "missing");
    let mapped =
        run(&registry, s, "vcs.patch.read", json!({ "path": file, "strip": 2, "baseDir": root.join("module") }));
    assert_eq!(mapped["files"][0]["relative"], "module/src/lib.go", "{mapped}");
    assert_eq!(mapped["files"][0]["state"], "clean");
    assert_eq!(mapped["baseDir"], "module");
    let applied = run(&registry, s, "vcs.patch.apply", json!({ "path": file, "strip": 2, "baseDir": "module" }));
    assert_eq!(applied["applied"], json!(["module/src/lib.go"]));
    assert!(std::fs::read_to_string(root.join("module/src/lib.go")).unwrap().contains("println"));
    let too_many = fails(&registry, s, "vcs.patch.read", json!({ "path": file, "strip": 4 }));
    assert!(too_many.contains("fewer folders"), "{too_many}");
    let outside = fails(&registry, s, "vcs.patch.read", json!({ "path": file, "baseDir": "/tmp" }));
    assert!(outside.contains("not in the repository"), "{outside}");
}

#[test]
fn a_failed_hunk_is_reported_and_left_to_merge() {
    let dir = test_repository();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("a.txt"), "one\ntwo\nthree\nfour\nfive\nsix\nseven\n").unwrap();
    git(&root, &["commit", "-q", "-am", "Seven"]);
    let data = tempfile::tempdir().unwrap();
    let (registry, mut session, _) = session(&root, data.path());
    let s = &mut session;
    let text = "--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n@@ -5,3 +5,3 @@\n five\n-six\n+SIX\n seven\n";
    std::fs::write(root.join("a.txt"), "one\ntwo\nthree\nfour\nfive\nmine\nseven\n").unwrap();
    let read = run(&registry, s, "vcs.patch.read", json!({ "text": text }));
    assert_eq!(read["files"][0]["state"], "conflict");
    assert_eq!(read["files"][0]["failed"], json!([{ "hunk": 2, "line": 5, "header": "@@ -5,3 +5,3 @@" }]));
    let diff = run(&registry, s, "vcs.patch.diff", json!({ "text": text, "file": root.join("a.txt") }));
    assert_eq!(diff["state"], "conflict");
    assert_eq!(diff["conflicts"], json!([{ "start": 5, "end": 10 }]), "{diff}");
    assert!(diff["after"].as_str().unwrap().contains("<<<<<<< Current\nmine\n=======\nSIX\n>>>>>>> Patch\n"));

    let applied = run(&registry, s, "vcs.patch.apply", json!({ "text": text }));
    assert_eq!(applied["conflicts"], json!(["a.txt"]), "{applied}");
    let told = s.state_mut::<GitWorkspace>().take_conflicts().unwrap();
    assert!(told.contains("Some hunks did not apply") && told.contains("a.txt"), "{told}");
    let conflicts = run(&registry, s, "vcs.conflict.list", json!({}));
    assert_eq!(conflicts["files"][0]["relative"], "a.txt", "{conflicts}");
    run(&registry, s, "vcs.conflict.open", json!({ "path": "a.txt" }));
    let merge = run(&registry, s, "vcs.conflict.get", json!({}));
    assert_eq!(merge["ours"]["text"], "one\ntwo\nthree\nfour\nfive\nmine\nseven\n", "{merge}");
    assert_eq!(merge["theirs"]["text"], "one\nTWO\nthree\nfour\nfive\nSIX\nseven\n");
    run(&registry, s, "vcs.conflict.resolve", json!({ "take": "theirs", "markResolved": true }));
    assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "one\nTWO\nthree\nfour\nfive\nSIX\nseven\n");
}

#[test]
fn a_git_patch_merges_three_way_from_the_blob_it_was_made_from() {
    let dir = test_repository();
    let root = dir.path().canonicalize().unwrap();
    let lines = |changed: &[(usize, &str)]| -> String {
        (1..=9)
            .map(|n| changed.iter().find(|(at, _)| *at == n).map_or(format!("l{n}\n"), |(_, text)| format!("{text}\n")))
            .collect()
    };
    std::fs::write(root.join("a.txt"), lines(&[])).unwrap();
    git(&root, &["commit", "-q", "-am", "Nine"]);
    let data = tempfile::tempdir().unwrap();
    let (registry, mut session, _) = session(&root, data.path());
    let s = &mut session;
    std::fs::write(root.join("a.txt"), lines(&[(3, "T3"), (7, "T7")])).unwrap();
    let made = run(&registry, s, "vcs.patch.create", json!({ "paths": ["a.txt"] }));
    git(&root, &["checkout", "--", "a.txt"]);
    std::fs::write(root.join("a.txt"), lines(&[(5, "O5")])).unwrap();
    let read = run(&registry, s, "vcs.patch.read", json!({ "text": made["text"] }));
    assert_eq!(read["files"][0]["state"], "merged", "{read}");
    assert_eq!(read["files"][0]["failed"][0]["hunk"], 1, "the hunk does not apply as it is");
    run(&registry, s, "vcs.patch.apply", json!({ "text": made["text"] }));
    assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), lines(&[(3, "T3"), (5, "O5"), (7, "T7")]));
}

#[test]
fn a_patch_goes_on_the_shelf_instead() {
    let dir = test_repository();
    let root = dir.path().canonicalize().unwrap();
    let data = tempfile::tempdir().unwrap();
    let (registry, mut session, _) = session(&root, data.path());
    let s = &mut session;
    let shelf = run(&registry, s, "vcs.shelf.import", json!({ "text": FORMAT_PATCH }));
    assert_eq!(shelf["name"], "Shout two");
    assert_eq!(shelf["files"].as_array().map(Vec::len), Some(2));
    assert_eq!(shelf["files"][1]["status"], "added");
    assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "one\ntwo\nthree\n", "the files are left alone");
    let diff = run(&registry, s, "vcs.shelf.diff", json!({ "id": shelf["id"], "path": "a.txt" }));
    assert_eq!(diff["after"], "one\nTWO\nthree\n");
    run(&registry, s, "vcs.shelf.unshelve", json!({ "id": shelf["id"] }));
    assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "one\nTWO\nthree\n");
    assert_eq!(std::fs::read_to_string(root.join("docs/new.md")).unwrap(), "# New\n");
    let named = run(
        &registry,
        s,
        "vcs.shelf.import",
        json!({ "text": FORMAT_PATCH, "paths": ["docs/new.md"], "name": "Docs" }),
    );
    assert_eq!((named["name"].as_str(), named["files"].as_array().map(Vec::len)), (Some("Docs"), Some(1)));
}

#[test]
fn paths_outside_the_project_are_refused() {
    let dir = test_repository();
    let root = dir.path().canonicalize().unwrap();
    let data = tempfile::tempdir().unwrap();
    let (registry, mut session, _) = session(&root, data.path());
    let s = &mut session;
    let victim = data.path().join("victim.txt");
    std::fs::write(&victim, "safe\n").unwrap();
    for text in [
        "--- a/../victim.txt\n+++ b/../victim.txt\n@@ -1 +1 @@\n-safe\n+owned\n".to_owned(),
        format!("--- {0}\n+++ {0}\n@@ -1 +1 @@\n-safe\n+owned\n", victim.display()),
        "diff --git a/.git/hooks/pre-commit b/.git/hooks/pre-commit\nnew file mode 100755\n--- /dev/null\n+++ b/.git/hooks/pre-commit\n@@ -0,0 +1 @@\n+echo owned\n".to_owned(),
        "diff --git a/x b/x\nrename from x\nrename to ../../x\n".to_owned(),
    ] {
        let message = fails(&registry, s, "vcs.patch.apply", json!({ "text": text }));
        assert!(message.contains("outside the project or in .git"), "{message}");
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(data.path(), root.join("link")).unwrap();
        let text = "--- a/link/victim.txt\n+++ b/link/victim.txt\n@@ -1 +1 @@\n-safe\n+owned\n";
        let read = run(&registry, s, "vcs.patch.read", json!({ "text": text }));
        assert_eq!(read["files"][0]["state"], "failed", "{read}");
        assert!(registry.dispatch(s, "vcs.patch.apply", json!({ "text": text })).is_err());
        std::os::unix::fs::symlink(".git", root.join("g")).unwrap();
        std::os::unix::fs::symlink("a.txt", root.join("l")).unwrap();
        for text in [
            "--- /dev/null\n+++ b/g/hooks/post-commit\n@@ -0,0 +1 @@\n+owned\n",
            "--- a/l\n+++ b/l\n@@ -1,3 +1,3 @@\n one\n-two\n+owned\n three\n",
            "diff --git a/out b/out\nnew file mode 120000\n--- /dev/null\n+++ b/out\n@@ -0,0 +1 @@\n+/etc\n\\ No newline at end of file\n",
            "diff --git a/a.txt b/a.txt\nold mode 100644\nnew mode 120000\n",
        ] {
            let read = run(&registry, s, "vcs.patch.read", json!({ "text": text }));
            assert_eq!(read["files"][0]["state"], "failed", "{read}");
            assert!(read["files"][0]["reason"].as_str().is_some_and(|reason| !reason.is_empty()), "{read}");
            assert!(registry.dispatch(s, "vcs.patch.apply", json!({ "text": text })).is_err(), "{text}");
        }
        assert!(!root.join(".git/hooks/post-commit").exists());
        assert!(!root.join("out").exists());
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "one\ntwo\nthree\n");
        assert!(std::fs::symlink_metadata(root.join("a.txt")).unwrap().file_type().is_file());
    }
    for evil in [".GIT/config", ".git\u{200C}/config", ".git./config", "git~1/config"] {
        let text = format!("--- /dev/null\n+++ b/{evil}\n@@ -0,0 +1 @@\n+owned\n");
        let message = fails(&registry, s, "vcs.patch.apply", json!({ "text": text }));
        assert!(message.contains("outside the project or in .git"), "{message}");
    }
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "safe\n");
    assert!(!root.join(".git/hooks/pre-commit").exists());
}

#[test]
fn what_git_apply_would_not_do_is_told() {
    let dir = test_repository();
    let root = dir.path().canonicalize().unwrap();
    let data = tempfile::tempdir().unwrap();
    let (registry, mut session, _) = session(&root, data.path());
    let s = &mut session;
    std::fs::write(root.join("b.txt"), "other\n").unwrap();
    let rename = "diff --git a/a.txt b/b.txt\nsimilarity index 100%\nrename from a.txt\nrename to b.txt\n";
    let read = run(&registry, s, "vcs.patch.read", json!({ "text": rename }));
    assert_eq!(read["files"][0]["state"], "exists", "git apply refuses a rename onto a file: {read}");
    std::fs::remove_file(root.join("b.txt")).unwrap();
    let read = run(&registry, s, "vcs.patch.read", json!({ "text": rename }));
    assert_eq!(read["files"][0]["state"], "clean", "{read}");

    std::fs::write(root.join("a.txt"), "zero\none\ntwo\nthree\n").unwrap();
    let start = "--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n-one\n+ONE\n two\n";
    let read = run(&registry, s, "vcs.patch.read", json!({ "text": start }));
    assert_eq!(read["files"][0]["state"], "clean", "{read}");
    assert_eq!(read["files"][0]["approximate"], json!([1]), "git apply wants it at the start: {read}");
    std::fs::write(root.join("a.txt"), "one\ntwo\nthree\n").unwrap();
    let read = run(&registry, s, "vcs.patch.read", json!({ "text": start }));
    assert_eq!(read["files"][0]["approximate"], json!([]), "{read}");

    let chmod = "diff --git a/a.txt b/a.txt\nold mode 100644\nnew mode 100755\n";
    let read = run(&registry, s, "vcs.patch.read", json!({ "text": chmod }));
    assert_eq!(read["files"][0]["executable"], true, "{read}");
    let bad_mode = "diff --git a/a.txt b/a.txt\nold mode 100644\nnew mode 100755\t1 x\n";
    let read = run(&registry, s, "vcs.patch.read", json!({ "text": bad_mode }));
    assert_eq!(read["files"][0]["executable"], Value::Null, "a mode that is not one is ignored: {read}");
}

#[test]
fn patches_are_written_and_read_in_other_encodings() {
    let dir = test_repository();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("ru.txt"), "привет\n").unwrap();
    std::fs::write(root.join("latin.txt"), b"caf\xe9\n").unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "Texts"]);
    let data = tempfile::tempdir().unwrap();
    let (registry, mut session, _) = session(&root, data.path());
    let s = &mut session;
    std::fs::write(root.join("ru.txt"), "мир\n").unwrap();
    std::fs::write(root.join("latin.txt"), b"na\xefve\n").unwrap();
    let out = data.path().join("cp1251.patch");
    let made =
        run(&registry, s, "vcs.patch.create", json!({ "paths": ["ru.txt"], "output": out, "encoding": "cp1251" }));
    assert_eq!(made["encoding"], "windows-1251");
    let bytes = std::fs::read(&out).unwrap();
    assert!(std::str::from_utf8(&bytes).is_err(), "not UTF-8");
    assert!(bytes.windows(3).any(|window| window == [0xec, 0xe8, 0xf0]), "мир in windows-1251");
    let latin = data.path().join("latin.patch");
    run(&registry, s, "vcs.patch.create", json!({ "paths": ["latin.txt"], "output": latin }));
    assert!(
        std::fs::read(&latin).unwrap().windows(5).any(|window| window == b"na\xefve"),
        "the Latin-1 bytes are kept"
    );
    git(&root, &["checkout", "--", "."]);
    git(&root, &["apply", "--check", latin.to_str().unwrap()]);
    let read = run(&registry, s, "vcs.patch.read", json!({ "path": out, "encoding": "windows-1251" }));
    assert_eq!(read["files"][0]["state"], "clean", "{read}");
    let undecoded = run(&registry, s, "vcs.patch.read", json!({ "path": out }));
    assert_eq!(undecoded["files"][0]["state"], "failed", "read as UTF-8 it does not match: {undecoded}");
    run(&registry, s, "vcs.patch.apply", json!({ "path": out, "encoding": "windows-1251" }));
    assert_eq!(std::fs::read_to_string(root.join("ru.txt")).unwrap(), "мир\n");
    run(&registry, s, "vcs.patch.apply", json!({ "path": latin }));
    assert_eq!(std::fs::read(root.join("latin.txt")).unwrap(), b"na\xefve\n");
}

#[test]
fn binary_files_and_modes_apply() {
    let dir = test_repository();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("run.sh"), "echo hi\n").unwrap();
    std::fs::write(root.join("logo.bin"), [1u8, 2, 3]).unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "Files"]);
    let data = tempfile::tempdir().unwrap();
    let (registry, mut session, _) = session(&root, data.path());
    let s = &mut session;
    std::fs::write(root.join("logo.bin"), [0u8, 255, 0, 255]).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root.join("run.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::write(root.join("run.sh"), "echo hello\n").unwrap();
    let made = run(&registry, s, "vcs.patch.create", json!({ "paths": ["logo.bin", "run.sh"] }));
    git(&root, &["checkout", "--", "."]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root.join("run.sh"), std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    let read = run(&registry, s, "vcs.patch.read", json!({ "text": made["text"] }));
    assert_eq!(read["files"][0]["state"], "binary", "{read}");
    let applied = run(&registry, s, "vcs.patch.apply", json!({ "text": made["text"] }));
    assert_eq!(applied["applied"].as_array().map(Vec::len), Some(2), "{applied}");
    assert_eq!(std::fs::read(root.join("logo.bin")).unwrap(), [0u8, 255, 0, 255]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(root.join("run.sh")).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "executable again: {mode:o}");
    }
}

#[test]
fn malformed_patches_are_explained() {
    let dir = test_repository();
    let root = dir.path().canonicalize().unwrap();
    let data = tempfile::tempdir().unwrap();
    let (registry, mut session, _) = session(&root, data.path());
    let s = &mut session;
    assert!(fails(&registry, s, "vcs.patch.read", json!({ "text": "hello" })).contains("no changes"));
    assert!(fails(&registry, s, "vcs.patch.read", json!({})).contains("Give a patch"));
    let cut =
        fails(&registry, s, "vcs.patch.read", json!({ "text": "--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,3 @@\n one\n" }));
    assert!(cut.contains("line 3"), "{cut}");
    assert!(
        fails(&registry, s, "vcs.patch.read", json!({ "text": FORMAT_PATCH, "encoding": "nope" })).contains("nope")
    );
    let not_there = fails(&registry, s, "vcs.patch.apply", json!({ "text": FORMAT_PATCH, "paths": ["b.txt"] }));
    assert!(not_there.contains("b.txt is not in the patch"), "{not_there}");
}
