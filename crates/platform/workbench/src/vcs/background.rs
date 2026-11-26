use std::sync::{Arc, Mutex};

use zen_core::{CommandError, Session};
use zen_vcs::{Repository, VcsError};

use super::remote::{self, OperationInfo};
use super::{CONFLICTS, message, repository, vcs};

pub(super) type Finishing = Box<dyn FnOnce(&mut Session) -> Result<(), CommandError> + Send>;

pub fn off_the_frame<T>(session: &mut Session, dispatch: impl FnOnce(&mut Session) -> T) -> T {
    vcs(session).off_the_frame = true;
    let answer = dispatch(session);
    vcs(session).off_the_frame = false;
    answer
}

pub(super) struct Step {
    pub(super) kind: &'static str,
    pub(super) title: String,
    pub(super) done: zen_i18n::Message,
}

impl Step {
    pub(super) fn new(kind: &'static str, title: String) -> Self {
        Step { kind, title, done: zen_i18n::Message::plain("") }
    }
}

pub(super) fn run<T, R>(
    session: &mut Session,
    step: Step,
    work: impl FnOnce(&Repository) -> Result<T, VcsError> + Send + 'static,
    finish: impl FnOnce(&mut Session, Result<T, VcsError>) -> Result<R, CommandError> + Send + 'static,
) -> Result<R, CommandError>
where
    T: Send + 'static,
    R: Default + 'static,
{
    if !vcs(session).off_the_frame {
        let repository = repository(session)?;
        let result = work(&repository);
        return finish(session, result);
    }
    let outcome = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&outcome);
    let Step { kind, title, done } = step;
    let job: remote::Work = Box::new(move |repository, _| {
        let result = work(repository);
        let answer = result.as_ref().map(|_| done).map_err(message);
        *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(result);
        answer
    });
    remote::start(session, kind, title, false, job)?;
    vcs(session).finishing = Some(Box::new(move |session| {
        let result = outcome.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
        result.map_or(Ok(()), |result| finish(session, result).map(drop))
    }));
    Ok(R::default())
}

pub(super) fn settled(session: &mut Session, mut operation: Option<OperationInfo>) -> Option<OperationInfo> {
    if operation.as_ref().is_some_and(|operation| operation.running) {
        return operation;
    }
    let Some(finishing) = vcs(session).finishing.take() else { return operation };
    let finished = finishing(session);
    if finished.is_err_and(|error| error.code.as_deref() == Some(CONFLICTS)) {
        if let Some(operation) = operation.as_mut() {
            operation.message = None;
        }
        if let Some(operation) = vcs(session).shared.operation().as_mut() {
            operation.message = None;
        }
    }
    operation
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use zen_core::{Registry, Session};

    use super::super::testing::*;
    use super::*;

    fn over(registry: &Registry, session: &mut Session) -> serde_json::Value {
        loop {
            let status = registry.dispatch(session, "vcs.operation.status", json!({})).unwrap();
            if status["running"] == false {
                return status;
            }
            std::thread::yield_now();
        }
    }

    fn slow_hook(root: &std::path::Path, hook: &str, gate: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;
        let file = root.join(".git/hooks").join(hook);
        let script = format!("#!/bin/sh\nwhile [ ! -e '{}' ]; do :; done\n", gate.display());
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, script).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_commit_off_the_frame_answers_before_its_hook_ends_and_records_after_it() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        let (registry, mut session) = super::super::tests::session(&root);
        let gates = tempfile::tempdir().unwrap();
        let gate = gates.path().join("open");
        slow_hook(&root, "pre-commit", &gate);
        std::fs::write(root.join("a.txt"), "changed\n").unwrap();
        let params = json!({ "message": "Slow", "paths": ["a.txt"] });
        let answer =
            off_the_frame(&mut session, |session| registry.dispatch(session, "vcs.commit.create", params)).unwrap();
        assert_eq!(answer["commit"], "", "not waited for");
        let status = registry.dispatch(&mut session, "vcs.operation.status", json!({})).unwrap();
        assert_eq!((status["kind"].as_str(), status["running"].as_bool()), (Some("commit"), Some(true)));
        assert!(registry.dispatch(&mut session, "vcs.history.list", json!({})).unwrap()["undo"].is_null());

        std::fs::write(&gate, "").unwrap();
        let status = over(&registry, &mut session);
        assert_eq!(status["succeeded"], true, "{status}");
        assert!(status["message"].as_str().is_some_and(|message| message.contains("Slow")), "{status}");
        assert_eq!(git(&root, &["log", "-1", "--format=%s"]).trim(), "Slow");
        let history = registry.dispatch(&mut session, "vcs.history.list", json!({})).unwrap();
        assert!(history["undo"].is_object(), "the undo entry comes when it is over: {history}");
    }

    #[test]
    fn the_same_commands_dispatched_by_agents_still_answer_when_they_are_over() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        let (registry, mut session) = super::super::tests::session(&root);
        std::fs::write(root.join("a.txt"), "changed\n").unwrap();
        let params = json!({ "message": "Now", "paths": ["a.txt"] });
        let answer = registry.dispatch(&mut session, "vcs.commit.create", params).unwrap();
        assert_eq!(answer["commit"].as_str().map(str::len), Some(40));
        assert_eq!(git(&root, &["log", "-1", "--format=%s"]).trim(), "Now");
    }

    #[test]
    fn a_failure_off_the_frame_is_the_outcome_of_the_operation() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        let (registry, mut session) = super::super::tests::session(&root);
        let params = json!({ "name": "no-such-branch" });
        off_the_frame(&mut session, |session| registry.dispatch(session, "vcs.branch.checkout", params)).unwrap();
        let status = over(&registry, &mut session);
        assert_eq!((status["kind"].as_str(), status["succeeded"].as_bool()), (Some("checkout"), Some(false)));
        assert!(status["message"].as_str().is_some_and(|message| !message.is_empty()), "{status}");
    }

    #[test]
    fn conflicts_off_the_frame_go_to_the_git_workspace_and_not_to_a_notification() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        let (registry, mut session) = super::super::tests::session(&root);
        git(&root, &["switch", "-q", "-c", "other"]);
        std::fs::write(root.join("a.txt"), "theirs\n").unwrap();
        git(&root, &["commit", "-q", "-am", "Theirs"]);
        git(&root, &["switch", "-q", "-"]);
        std::fs::write(root.join("a.txt"), "ours\n").unwrap();
        git(&root, &["commit", "-q", "-am", "Ours"]);
        let params = json!({ "name": "other" });
        off_the_frame(&mut session, |session| registry.dispatch(session, "vcs.branch.merge", params)).unwrap();
        let status = over(&registry, &mut session);
        assert_eq!(status["succeeded"], false, "{status}");
        assert!(status["message"].is_null(), "{status}");
        let told = session.state_mut::<super::super::workspace::GitWorkspace>().take_conflicts();
        assert!(told.is_some_and(|message| message.contains("a.txt")));
    }
}
