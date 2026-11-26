use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};

use zen_vcs::git::{self, CloneRequest, set_gate};
use zen_vcs::{Repository, VcsError};

static ONE_GATE: Mutex<()> = Mutex::new(());

fn raw_git(dir: &Path, args: &[&str]) {
    let done = Command::new("git").current_dir(dir).args(args).env("GIT_CONFIG_GLOBAL", "/dev/null").status().unwrap();
    assert!(done.success(), "git {args:?}");
}

#[cfg(unix)]
fn hostile_repository(dir: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let (by_filter, by_hook) = (dir.join("ran-filter"), dir.join("ran-hook"));
    raw_git(dir, &["init", "-q", "-b", "main"]);
    raw_git(dir, &["config", "user.name", "A"]);
    raw_git(dir, &["config", "user.email", "a@e"]);
    raw_git(dir, &["config", "core.hooksPath", ".git/hooks"]);
    std::fs::write(dir.join(".gitattributes"), "*.txt filter=evil\n").unwrap();
    raw_git(dir, &["add", ".gitattributes"]);
    raw_git(dir, &["commit", "-q", "-m", "First"]);
    raw_git(dir, &["config", "filter.evil.clean", &format!("touch {}; cat", by_filter.display())]);
    let hook = dir.join(".git/hooks/pre-commit");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(&hook, format!("#!/bin/sh\ntouch {}\n", by_hook.display())).unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(dir.join("a.txt"), "changed\n").unwrap();
    (by_filter, by_hook)
}

#[cfg(unix)]
#[test]
fn with_the_gate_shut_no_git_call_runs_a_filter_driver_or_a_hook_of_the_repository() {
    let _gate = ONE_GATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let (by_filter, by_hook) = hostile_repository(&root);
    let known = Repository::discover(&root).expect("found while the gate is open");

    set_gate(Some(Arc::new(|_: &Path| false)));
    assert!(!git::allowed(&root));
    assert!(Repository::discover(&root).is_none(), "nothing is discovered");
    assert!(matches!(known.status(), Err(VcsError::NotTrusted)), "gix reads pass the gate too");
    assert!(matches!(known.head(), Err(VcsError::NotTrusted)));
    assert!(matches!(git::run(&root, &["status", "--porcelain"], None), Err(VcsError::NotTrusted)));
    assert!(matches!(git::run(&root, &["add", "a.txt"], None), Err(VcsError::NotTrusted)));
    assert!(matches!(known.stage(&["a.txt".to_owned()]), Err(VcsError::NotTrusted)));
    assert!(matches!(known.commit("Evil", &["a.txt".to_owned()], false), Err(VcsError::NotTrusted)));
    assert!(!by_filter.exists(), "the filter driver did not run");
    assert!(!by_hook.exists(), "the hook did not run");

    set_gate(None);
    assert!(Repository::discover(&root).is_some(), "the gate opens without anything being reopened");
    known.commit("Now", &["a.txt".to_owned()], false).unwrap();
    assert!(by_filter.exists() && by_hook.exists(), "this repository does run them once the gate is open");
}

#[cfg(unix)]
#[test]
fn with_the_gate_shut_a_new_repository_can_be_made_and_stays_shut_afterwards() {
    use std::os::unix::fs::PermissionsExt;
    let _gate = ONE_GATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let origin = root.join("origin");
    std::fs::create_dir(&origin).unwrap();
    raw_git(&origin, &["init", "-q", "-b", "main"]);
    raw_git(&origin, &["config", "user.name", "A"]);
    raw_git(&origin, &["config", "user.email", "a@e"]);
    std::fs::write(origin.join("a.txt"), "a\n").unwrap();
    raw_git(&origin, &["add", "a.txt"]);
    raw_git(&origin, &["commit", "-q", "-m", "First"]);
    let parent = root.join("hostile-parent");
    std::fs::create_dir(&parent).unwrap();
    let (by_filter, by_hook) = hostile_repository(&parent);
    let by_checkout = parent.join("ran-checkout");
    let hook = parent.join(".git/hooks/post-checkout");
    std::fs::write(&hook, format!("#!/bin/sh\ntouch {}\n", by_checkout.display())).unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();

    set_gate(Some(Arc::new(|_: &Path| false)));
    let cloned = parent.join("cloned");
    let request = CloneRequest { url: format!("file://{}", origin.display()), folder: cloned.clone(), depth: None };
    git::clone(&request, &mut |_| {}).unwrap();
    assert!(cloned.join("a.txt").is_file());
    assert!(matches!(git::run(&cloned, &["status", "--porcelain"], None), Err(VcsError::NotTrusted)));
    let fresh = root.join("fresh");
    std::fs::create_dir(&fresh).unwrap();
    git::init(&fresh).unwrap();
    assert!(fresh.join(".git").is_dir());
    assert!(matches!(git::run(&fresh, &["status", "--porcelain"], None), Err(VcsError::NotTrusted)));
    set_gate(None);

    assert!(!by_filter.exists() && !by_hook.exists() && !by_checkout.exists(), "the folder around it runs nothing");
}

#[test]
fn a_clone_fetches_no_submodule_a_repository_names() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let commit = |folder: &Path, file: &str| {
        raw_git(folder, &["init", "-q", "-b", "main"]);
        raw_git(folder, &["config", "user.name", "A"]);
        raw_git(folder, &["config", "user.email", "a@e"]);
        std::fs::write(folder.join(file), "a\n").unwrap();
        raw_git(folder, &["add", "."]);
        raw_git(folder, &["commit", "-q", "-m", "First"]);
    };
    let (inner, outer) = (root.join("inner"), root.join("outer"));
    std::fs::create_dir(&inner).unwrap();
    std::fs::create_dir(&outer).unwrap();
    commit(&inner, "in.txt");
    commit(&outer, "out.txt");
    let inner_url = format!("file://{}", inner.display());
    raw_git(&outer, &["-c", "protocol.file.allow=always", "submodule", "add", "-q", &inner_url, "sub"]);
    raw_git(&outer, &["commit", "-q", "-m", "Second"]);
    let cloned = root.join("cloned");
    let request = CloneRequest { url: format!("file://{}", outer.display()), folder: cloned.clone(), depth: None };
    git::clone(&request, &mut |_| {}).unwrap();
    assert!(cloned.join("out.txt").is_file());
    assert!(cloned.join(".gitmodules").is_file());
    assert!(!cloned.join("sub/in.txt").exists(), "the submodule waits for the person's trust");
    assert!(!cloned.join(".git/modules").exists());
}
