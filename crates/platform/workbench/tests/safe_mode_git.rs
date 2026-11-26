use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};
use zen_core::{DispatchError, Registry, Session};
use zen_workbench::build_tools::BuildEnv;

static ONE_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn raw_git(dir: &Path, args: &[&str]) {
    let done = Command::new("git").current_dir(dir).args(args).env("GIT_CONFIG_GLOBAL", "/dev/null").status().unwrap();
    assert!(done.success(), "git {args:?}");
}

fn refusal(answer: Result<Value, DispatchError>) -> (String, String) {
    match answer {
        Err(DispatchError::Failed { error, .. }) => (error.code.unwrap_or_default(), error.message),
        other => panic!("not refused: {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn git_is_off_in_an_untrusted_project_and_starts_when_the_person_trusts_it() {
    let _gate = ONE_GATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    use std::os::unix::fs::PermissionsExt;
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap().join("shop");
    std::fs::create_dir(&root).unwrap();
    raw_git(&root, &["init", "-q", "-b", "main"]);
    raw_git(&root, &["config", "user.name", "A"]);
    raw_git(&root, &["config", "user.email", "a@e"]);
    std::fs::write(root.join(".gitattributes"), "*.txt filter=evil\n").unwrap();
    raw_git(&root, &["add", ".gitattributes"]);
    raw_git(&root, &["commit", "-q", "-m", "First"]);
    let (by_filter, by_hook) = (dir.path().join("ran-filter"), dir.path().join("ran-hook"));
    raw_git(&root, &["config", "filter.evil.clean", &format!("touch {}; cat", by_filter.display())]);
    let hook = root.join(".git/hooks/pre-commit");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(&hook, format!("#!/bin/sh\ntouch {}\n", by_hook.display())).unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(root.join("a.txt"), "changed\n").unwrap();

    let env = BuildEnv::default();
    env.set_data_dir(Some(dir.path().join("data")));
    zen_workbench::trust::install_git_gate(env.clone());
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    session.set_state(env);
    registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();

    let info = registry.dispatch(&mut session, "vcs.repository.info", json!({})).unwrap();
    assert!(info["root"].is_null(), "the window sees no repository: {info}");
    assert!(info["error"].as_str().unwrap().contains("safe mode"), "and is told why: {info}");
    let commit = json!({ "message": "Evil", "paths": ["a.txt"] });
    let (code, person) = refusal(registry.dispatch(&mut session, "vcs.commit.create", commit.clone()));
    assert_eq!(code, "notTrusted");
    assert!(person.contains("safe mode"), "{person}");
    for (command, params) in [
        ("vcs.commit.create", commit.clone()),
        ("vcs.status.refresh", json!({})),
        ("vcs.branch.list", json!({})),
        ("vcs.log.list", json!({})),
    ] {
        let answer =
            zen_core::macros::as_agent(&mut session, |session| registry.dispatch(session, command, params.clone()));
        if let Err(DispatchError::Failed { error, .. }) = answer {
            assert_eq!(error.code.as_deref(), Some("notTrusted"), "{command}: {}", error.message);
            assert!(error.message.contains("window"), "{command} says what the person does: {}", error.message);
        }
    }
    assert!(!by_filter.exists() && !by_hook.exists(), "no filter driver or hook of the repository ran");

    registry.dispatch(&mut session, "project.trust.grant", json!({})).unwrap();
    let info = registry.dispatch(&mut session, "vcs.repository.info", json!({})).unwrap();
    assert!(info["root"].is_string(), "Git starts without reopening the project: {info}");
    assert!(info["error"].is_null(), "{info}");
    registry.dispatch(&mut session, "vcs.commit.create", commit).unwrap();
    assert!(by_filter.exists() && by_hook.exists(), "they run once the person trusted the project");
}

#[test]
fn a_clone_and_a_person_s_git_project_are_made_in_folders_nobody_trusted() {
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

    let env = BuildEnv::default();
    env.set_data_dir(Some(root.join("data")));
    zen_workbench::trust::install_git_gate(env.clone());
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    session.set_state(env.clone());

    let cloned = root.join("projects/cloned");
    let params = json!({ "url": format!("file://{}", origin.display()), "directory": cloned, "waitMs": 30000 });
    let answer = registry.dispatch(&mut session, "project.fromVcs.clone", params).unwrap();
    assert_eq!(answer["state"], "ready", "{answer}");
    assert_eq!(answer["trusted"], false, "a clone is never trusted by itself");
    assert!(!zen_workbench::trust::is_trusted(&env, &cloned));
    assert!(!zen_vcs::git::allowed(&cloned), "and Git stays off in it");

    let created = root.join("projects/created");
    let params = json!({ "generator": "empty", "location": created, "git": true, "waitMs": 30000 });
    let answer = registry.dispatch(&mut session, "project.new.create", params).unwrap();
    assert_eq!(answer["state"], "ready", "{answer}");
    assert_eq!(answer["trusted"], true, "a project the person creates is trusted");
    assert!(created.join(".git").is_dir());
    assert!(zen_vcs::git::allowed(&created));
}
