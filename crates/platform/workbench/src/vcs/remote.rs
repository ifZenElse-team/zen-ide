use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::{Message, t};
use zen_vcs::Repository;
use zen_vcs::git::Progress;

use super::{Shared, Vcs, repository, vcs};

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(UpdateProject)?;
    registry.register(Fetch)?;
    registry.register(UpdateBranch)?;
    registry.register(PushOutgoing)?;
    registry.register(Push)?;
    registry.register(PushTag)?;
    registry.register(DeleteRemoteBranch)?;
    registry.register(OperationStatus)?;
    Ok(())
}

/// A background operation: where it is, and how it ended.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct OperationInfo {
    /// Raised for every operation started, so that a window shows each outcome once.
    pub id: u64,
    /// `update`, `fetch`, `push`, or a local one: `checkout`, `merge`, `rebase`, `cherryPick`,
    /// `revert`, `reset` or `commit`.
    pub kind: String,
    /// What the status bar shows while it runs: `Updating…`.
    pub title: String,
    /// Whether it is still running.
    pub running: bool,
    /// How far the current phase is, from 0 to 1, when git says.
    pub fraction: Option<f32>,
    /// The phase or the last line git printed.
    pub detail: String,
    /// When it is over: whether it succeeded.
    pub succeeded: Option<bool>,
    /// When it is over: what to tell the user (`Pushed 2 commits to origin/main`, or git's error).
    #[serde(serialize_with = "worded", deserialize_with = "kept")]
    #[schemars(with = "Option<String>")]
    pub message: Option<Message>,
}

fn worded<S: serde::Serializer>(message: &Option<Message>, serializer: S) -> Result<S::Ok, S::Error> {
    message.as_ref().map(Message::text).serialize(serializer)
}

fn kept<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<Message>, D::Error> {
    Ok(Option::<String>::deserialize(deserializer)?.map(Message::plain))
}

pub(super) type Work = Box<dyn FnOnce(&Repository, &mut dyn FnMut(Progress)) -> Result<Message, String> + Send>;

pub(super) fn start(
    session: &mut Session,
    kind: &str,
    title: String,
    wait: bool,
    work: Work,
) -> Result<OperationInfo, CommandError> {
    let repository = repository(session)?;
    if vcs(session).shared.operation().as_ref().is_some_and(|operation| operation.running) {
        return Err(CommandError::new(t!("vcs-error-busy")));
    }
    let task = session.state_mut::<crate::tasks::Tasks>().start(&title, "vcs", false);
    let vcs: &mut Vcs = vcs(session);
    let shared = Arc::clone(&vcs.shared);
    let id = {
        let mut operation = shared.operation();
        if operation.as_ref().is_some_and(|operation| operation.running) {
            return Err(CommandError::new(t!("vcs-error-busy")));
        }
        let id = operation.as_ref().map_or(1, |operation| operation.id + 1);
        *operation =
            Some(OperationInfo { id, kind: kind.to_owned(), title, running: true, ..OperationInfo::default() });
        id
    };
    #[cfg(not(target_arch = "wasm32"))]
    let run = {
        let shared = Arc::clone(&shared);
        let programs = zen_run::Programs::default();
        if matches!(kind, "fetch" | "push") {
            task.cancel_kills(Arc::new(programs.clone()));
        } else {
            task.attach(Arc::new(programs.clone()));
        }
        move || {
            let watcher: zen_vcs::git::Watcher = Box::new(move |spawned| match spawned {
                zen_vcs::git::Spawned::Started(pid) => programs.add(pid),
                zen_vcs::git::Spawned::Ended(pid) => programs.remove(pid),
            });
            zen_vcs::git::watched(watcher, || run_operation(&shared, &repository, id, work, &task));
        }
    };
    #[cfg(target_arch = "wasm32")]
    let run = {
        let shared = Arc::clone(&shared);
        move || run_operation(&shared, &repository, id, work, &task)
    };
    #[cfg(not(target_arch = "wasm32"))]
    if wait {
        run();
    } else {
        std::thread::spawn(run);
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = wait;
        run();
    }
    if wait {
        vcs.settle();
    }
    Ok(shared.operation().clone().unwrap_or_default())
}

fn run_operation(
    shared: &Arc<Shared>,
    repository: &Arc<Repository>,
    id: u64,
    work: Work,
    task: &crate::tasks::TaskHandle,
) {
    let mut last_wake = std::time::Instant::now();
    let mut last_line = String::new();
    let mut progress = |progress: Progress| {
        if !progress.line.is_empty() && progress.line != last_line {
            let same_phase = !progress.phase.is_empty() && last_line.starts_with(&progress.phase);
            if !same_phase {
                task.log(&progress.line);
            }
            last_line = progress.line.clone();
        }
        let detail = if progress.phase.is_empty() { progress.line.clone() } else { progress.phase.clone() };
        task.progress(progress.fraction, &detail);
        if let Some(operation) = shared.operation().as_mut().filter(|operation| operation.id == id) {
            operation.fraction = progress.fraction;
            operation.detail = detail;
        }
        if last_wake.elapsed() > std::time::Duration::from_millis(100) {
            last_wake = std::time::Instant::now();
            shared.wake();
        }
    };
    let result = work(repository, &mut progress);
    if let Some(operation) = shared.operation().as_mut().filter(|operation| operation.id == id) {
        operation.running = false;
        operation.fraction = None;
        let (succeeded, message) = match result {
            _ if task.cancel_requested() => (false, Message::new("workbench-task-cancelled")),
            Ok(message) => (true, message),
            Err(message) => (false, Message::plain(message)),
        };
        operation.succeeded = Some(succeeded);
        task.finish(if succeeded { Ok(message.text()) } else { Err(message.text()) });
        operation.message = Some(message);
    }
    recompute(shared, repository);
    shared.wake();
}

fn recompute(shared: &Arc<Shared>, repository: &Arc<Repository>) {
    let stamp = repository.stamp();
    let (head, status, branches) = (repository.head(), repository.status(), repository.branches());
    let mut snapshot = shared.snapshot();
    if let (Ok(head), Ok(status), Ok(branches)) = (head, status, branches) {
        snapshot.head = head;
        snapshot.status = status;
        snapshot.branches = branches;
        snapshot.stamp = stamp;
        snapshot.refs_stamp = stamp;
        snapshot.revision += 1;
    }
}

/// Whether to wait for an operation.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct WaitParams {
    /// Wait until it is over and answer with its outcome; otherwise it runs in the background and
    /// vcs.operation.status follows it.
    pub wait: bool,
}

/// How Update Project brings in the remote's commits.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateParams {
    /// Rebase the local commits onto the remote's instead of merging (the Update Project dialog's
    /// choice).
    pub rebase: bool,
    /// Only fast-forward: refuse, leaving everything as it was, when the branch has commits of its
    /// own (not with `rebase`).
    pub fast_forward_only: bool,
    /// Wait until it is over.
    pub wait: bool,
}

impl UpdateParams {
    pub(super) fn mode(&self) -> Result<zen_vcs::PullMode, CommandError> {
        match (self.rebase, self.fast_forward_only) {
            (true, true) => Err(CommandError::new("`rebase` and `fastForwardOnly` cannot both be set")),
            (true, false) => Ok(zen_vcs::PullMode::Rebase),
            (false, true) => Ok(zen_vcs::PullMode::FastForwardOnly),
            (false, false) => Ok(zen_vcs::PullMode::Merge),
        }
    }
}

pub struct UpdateProject;

impl Command for UpdateProject {
    type Params = UpdateParams;
    type Output = OperationInfo;
    const ID: &'static str = "vcs.project.update";
    const TITLE: &'static str = "Update Project…";
    const DESCRIPTION: &'static str = "Fetches the current branch's upstream and merges it in, a fast-forward when it can be one \
         (or with `fastForwardOnly` only a fast-forward, refused when the branch has commits of its \
         own; or with `rebase` rebases onto it), in the background unless `wait` is set; \
         vcs.operation.status reports the progress and the outcome.";

    fn run(&self, session: &mut Session, params: UpdateParams) -> Result<OperationInfo, CommandError> {
        let mode = params.mode()?;
        start(
            session,
            "update",
            t!("vcs-progress-update"),
            params.wait,
            Box::new(move |repository, progress| {
                let before = repository.head().ok().and_then(|head| head.commit);
                repository.update(mode, progress).map_err(|error| super::message(&error))?;
                let after = repository.head().ok().and_then(|head| head.commit);
                if before == after {
                    return Ok(Message::new("vcs-update-up-to-date"));
                }
                let commits = match (before, after) {
                    (Some(before), Some(after)) => repository.count_between(&before, &after).unwrap_or(0),
                    _ => 0,
                };
                Ok(Message::new("vcs-update-done").with_number("count", commits as f64))
            }),
        )
    }
}

pub struct Fetch;

impl Command for Fetch {
    type Params = WaitParams;
    type Output = OperationInfo;
    const ID: &'static str = "vcs.remote.fetch";
    const TITLE: &'static str = "Fetch";
    const DESCRIPTION: &'static str =
        "Fetches every remote's branches, in the background unless `wait` is set; the local branches stay as they are.";

    fn run(&self, session: &mut Session, params: WaitParams) -> Result<OperationInfo, CommandError> {
        start(
            session,
            "fetch",
            t!("vcs-progress-fetch"),
            params.wait,
            Box::new(|repository, progress| {
                repository.fetch(progress).map_err(|error| super::message(&error))?;
                Ok(Message::new("vcs-fetch-done"))
            }),
        )
    }
}

/// A local branch to bring up to date.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBranchParams {
    /// The local branch.
    pub name: String,
    /// Wait until it is over.
    #[serde(default)]
    pub wait: bool,
}

pub struct UpdateBranch;

impl Command for UpdateBranch {
    type Params = UpdateBranchParams;
    type Output = OperationInfo;
    const ID: &'static str = "vcs.branch.update";
    const TITLE: &'static str = "Update Selected";
    const DESCRIPTION: &'static str = "Brings a local branch up to date with its upstream: a branch that is not checked out is \
         fast-forwarded from the remote, the current one is updated as Update Project does; in the background \
         unless `wait` is set.";

    fn run(&self, session: &mut Session, params: UpdateBranchParams) -> Result<OperationInfo, CommandError> {
        let name = params.name;
        start(
            session,
            "update",
            t!("vcs-progress-update"),
            params.wait,
            Box::new(move |repository, progress| {
                repository.update_branch(&name, progress).map_err(|error| super::message(&error))?;
                Ok(Message::new("vcs-branch-updated").with("branch", name.as_str()))
            }),
        )
    }
}

/// A commit, as lists show it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CommitInfo {
    /// The full id, as hex.
    pub id: String,
    /// The first eight characters of the id.
    pub short_id: String,
    /// The author's name.
    pub author: String,
    /// The author's e-mail.
    pub email: String,
    /// When it was authored, in seconds since 1970.
    pub time: i64,
    /// The author's time zone, in seconds east of UTC: dates show in it.
    pub offset: i32,
    /// The message's first line.
    pub subject: String,
}

impl From<&zen_vcs::Commit> for CommitInfo {
    fn from(commit: &zen_vcs::Commit) -> Self {
        CommitInfo {
            id: commit.id.clone(),
            short_id: commit.id.chars().take(8).collect(),
            author: commit.author.clone(),
            email: commit.email.clone(),
            time: commit.time,
            offset: commit.offset,
            subject: commit.subject.clone(),
        }
    }
}

/// What Push would send.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Outgoing {
    /// The local branch.
    pub branch: String,
    /// The remote it goes to (`origin`).
    pub remote: String,
    /// The branch on the remote (`main`).
    pub target: String,
    /// Whether the remote does not have the branch yet: it is created and tracked.
    pub new_branch: bool,
    /// The last commit known of the remote branch; give it to `vcs.remote.push` as `remoteTip`.
    pub remote_tip: Option<String>,
    /// The commits to push, newest first.
    pub commits: Vec<CommitInfo>,
}

/// Which branch a push is about.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct OutgoingParams {
    /// A local branch; the current one when absent.
    pub branch: Option<String>,
}

fn pushed_branch(branch: &Option<String>) -> Option<String> {
    branch.as_deref().map(str::trim).filter(|branch| !branch.is_empty()).map(str::to_owned)
}

pub struct PushOutgoing;

impl Command for PushOutgoing {
    type Params = OutgoingParams;
    type Output = Outgoing;
    const ID: &'static str = "vcs.push.outgoing";
    const TITLE: &'static str = "Push…";
    const DESCRIPTION: &'static str = "Tells where Push would send the current branch (or the local branch `branch`): the remote \
         and branch, new or not, and lists the commits it would send.";

    fn run(&self, session: &mut Session, params: OutgoingParams) -> Result<Outgoing, CommandError> {
        let repository = repository(session)?;
        let outgoing = repository
            .outgoing_of(pushed_branch(&params.branch).as_deref())
            .map_err(|error| CommandError::new(super::message(&error)))?;
        Ok(Outgoing {
            branch: outgoing.branch,
            remote: outgoing.remote,
            target: outgoing.target,
            new_branch: outgoing.new_branch,
            remote_tip: outgoing.remote_tip,
            commits: outgoing.commits.iter().map(CommitInfo::from).collect(),
        })
    }
}

/// How to push.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PushParams {
    /// Force Push: replace the remote branch even if it has commits the local one does not
    /// (`--force-with-lease --force-if-includes`: refused when the remote branch has commits that
    /// were never in the local branch).
    pub force: bool,
    /// With `force`, the remote branch's commit the user saw (`remoteTip` of `vcs.push.outgoing`):
    /// refused when a fetch has moved it since.
    pub remote_tip: Option<String>,
    /// A local branch to push instead of the current one (the Git workspace's branch menu).
    pub branch: Option<String>,
    /// Wait until it is over.
    pub wait: bool,
}

pub struct Push;

impl Command for Push {
    type Params = PushParams;
    type Output = OperationInfo;
    const ID: &'static str = "vcs.remote.push";
    const TITLE: &'static str = "Push";
    const DESCRIPTION: &'static str = "Pushes the current branch (or the local branch `branch`) to its upstream, creating and \
         tracking the remote branch when it has none, in the background unless `wait` is set.";

    fn run(&self, session: &mut Session, params: PushParams) -> Result<OperationInfo, CommandError> {
        let (force, seen) = (params.force, params.remote_tip);
        let branch = pushed_branch(&params.branch);
        start(
            session,
            "push",
            t!("vcs-progress-push"),
            params.wait,
            Box::new(move |repository, progress| {
                let outgoing = repository.outgoing_of(branch.as_deref()).map_err(|error| super::message(&error))?;
                if force && seen.is_some() && seen != outgoing.remote_tip {
                    let target = format!("{}/{}", outgoing.remote, outgoing.target);
                    return Err(t!("vcs-push-remote-moved", branch = target.as_str()));
                }
                repository.push(&outgoing, force, progress).map_err(|error| super::message(&error))?;
                let target = format!("{}/{}", outgoing.remote, outgoing.target);
                Ok(if outgoing.commits.is_empty() && outgoing.new_branch {
                    Message::new("vcs-push-new-branch").with("branch", target.as_str())
                } else if outgoing.commits.is_empty() {
                    Message::new("vcs-push-nothing")
                } else {
                    Message::new("vcs-push-done")
                        .with_number("count", outgoing.commits.len() as f64)
                        .with("branch", target.as_str())
                })
            }),
        )
    }
}

/// A tag to push.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PushTagParams {
    /// The tag: `v1.0`.
    pub name: String,
    /// The remote; `origin` (or the only remote) when absent.
    #[serde(default)]
    pub remote: Option<String>,
    /// Wait until it is over.
    #[serde(default)]
    pub wait: bool,
}

pub struct PushTag;

impl Command for PushTag {
    type Params = PushTagParams;
    type Output = OperationInfo;
    const ID: &'static str = "vcs.tag.push";
    const TITLE: &'static str = "Push Tag";
    const DESCRIPTION: &'static str = "Pushes a tag to a remote (`origin` or the only remote unless `remote` says), in the \
         background unless `wait` is set; vcs.operation.status reports the progress and the outcome.";

    fn run(&self, session: &mut Session, params: PushTagParams) -> Result<OperationInfo, CommandError> {
        let name = params.name.trim().to_owned();
        if name.is_empty() {
            return Err(CommandError::new(t!("vcs-error-tag-name")));
        }
        let remote = params.remote;
        start(
            session,
            "push",
            t!("vcs-progress-push"),
            params.wait,
            Box::new(move |repository, progress| {
                let remote =
                    repository.push_tag(&name, remote.as_deref(), progress).map_err(|error| super::message(&error))?;
                Ok(Message::new("vcs-push-tag-done").with("tag", name.as_str()).with("remote", remote.as_str()))
            }),
        )
    }
}

/// A remote branch to delete.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteRemoteParams {
    /// The remote branch: `origin/feature`.
    pub name: String,
    /// Wait until it is over.
    #[serde(default)]
    pub wait: bool,
}

pub struct DeleteRemoteBranch;

impl Command for DeleteRemoteBranch {
    type Params = DeleteRemoteParams;
    type Output = OperationInfo;
    const ID: &'static str = "vcs.branch.deleteRemote";
    const TITLE: &'static str = "Delete Remote Branch";
    const DESCRIPTION: &'static str = "Deletes a branch on its remote (`origin/feature`: `feature` on `origin`) and forgets the \
         remote-tracking branch, in the background unless `wait` is set. The Git workspace asks first; \
         this command does not. Local branches stay.";

    fn run(&self, session: &mut Session, params: DeleteRemoteParams) -> Result<OperationInfo, CommandError> {
        let name = params.name.trim().to_owned();
        if name.is_empty() {
            return Err(CommandError::new(t!("vcs-error-branch-name")));
        }
        start(
            session,
            "push",
            t!("vcs-progress-delete-remote"),
            params.wait,
            Box::new(move |repository, progress| {
                repository.delete_remote_branch(&name, progress).map_err(|error| super::message(&error))?;
                Ok(Message::new("vcs-remote-branch-deleted").with("branch", name.as_str()))
            }),
        )
    }
}

pub struct OperationStatus;

impl Command for OperationStatus {
    type Params = NoParams;
    type Output = Option<OperationInfo>;
    const ID: &'static str = "vcs.operation.status";
    const TITLE: &'static str = "Background Tasks";
    const DESCRIPTION: &'static str = "The version control operation running in the background (Update Project, Fetch, Push) \
         with its progress, or the last one with its outcome; null when there was none.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Option<OperationInfo>, CommandError> {
        let project = session.state::<crate::Workbench>().and_then(crate::Workbench::project).map(ToOwned::to_owned);
        let vcs = session.state_mut::<Vcs>();
        vcs.repository_for(project.as_deref());
        let operation = vcs.shared.operation().clone();
        Ok(super::background::settled(session, operation))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::testing::*;
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_fetch_is_cancelled_with_the_programs_it_started_and_an_update_cannot_be() {
        use std::os::unix::fs::PermissionsExt;
        isolate();
        let remote = tempfile::tempdir().unwrap();
        git(remote.path(), &["init", "-q", "--bare", "-b", "main"]);
        let local = test_repository();
        let root = local.path().canonicalize().unwrap();
        git(&root, &["remote", "add", "origin", remote.path().to_str().unwrap()]);
        let scripts = tempfile::tempdir().unwrap();
        let (slow, started) = (scripts.path().join("slow"), scripts.path().join("started"));
        std::fs::write(&slow, format!("#!/bin/sh\ntouch '{}'\nsleep 60\n", started.display())).unwrap();
        std::fs::set_permissions(&slow, std::fs::Permissions::from_mode(0o755)).unwrap();
        git(&root, &["config", "remote.origin.uploadpack", slow.to_str().unwrap()]);
        let (registry, mut session) = super::super::tests::session(&root);

        registry.dispatch(&mut session, "vcs.remote.fetch", json!({})).unwrap();
        while !started.exists() {
            std::thread::yield_now();
        }
        let list = registry.dispatch(&mut session, "tasks.background.list", json!({})).unwrap();
        let task = list["tasks"].as_array().unwrap().iter().find(|task| task["source"] == "vcs").unwrap();
        assert_eq!(task["cancellable"], true, "{task}");
        registry.dispatch(&mut session, "tasks.background.cancel", json!({ "id": task["id"] })).unwrap();
        let status = loop {
            let status = registry.dispatch(&mut session, "vcs.operation.status", json!({})).unwrap();
            if status["running"] == false {
                break status;
            }
            std::thread::yield_now();
        };
        assert_eq!((&status["succeeded"], &status["message"]), (&json!(false), &json!(t!("workbench-task-cancelled"))));
        let log = registry.dispatch(&mut session, "tasks.log.read", json!({ "id": task["id"] })).unwrap();
        assert_eq!(log["task"]["state"], "cancelled", "{log}");

        std::fs::write(&slow, "#!/bin/sh\nexit 1\n").unwrap();
        registry.dispatch(&mut session, "vcs.project.update", json!({})).unwrap();
        let list = registry.dispatch(&mut session, "tasks.background.list", json!({})).unwrap();
        let update = list["tasks"].as_array().unwrap().iter().rfind(|task| task["source"] == "vcs").unwrap();
        assert!(update.get("cancellable").is_none(), "stopping a merge half-way is not safe: {update}");
    }

    #[test]
    fn a_push_tells_an_agent_its_outcome_in_english_whatever_language_started_it() {
        isolate();
        zen_i18n::register(crate::LOCALES);
        let remote = tempfile::tempdir().unwrap();
        git(remote.path(), &["init", "-q", "--bare", "-b", "main"]);
        let local = test_repository();
        let root = local.path().canonicalize().unwrap();
        git(&root, &["remote", "add", "origin", remote.path().to_str().unwrap()]);
        let (registry, mut session) = super::super::tests::session(&root);
        registry.dispatch(&mut session, "vcs.remote.push", json!({ "wait": true })).unwrap();
        std::fs::write(root.join("b.txt"), "b\n").unwrap();
        git(&root, &["add", "b.txt"]);
        let committed = zen_i18n::with_language("ru", || {
            let params = json!({ "message": "Second", "paths": ["b.txt"], "push": true, "wait": true });
            registry.dispatch(&mut session, "vcs.commit.create", params).unwrap()
        });
        assert_eq!(committed["push"]["succeeded"], true, "the commit waits for its push: {committed}");
        let status = zen_i18n::with_language("en", || {
            registry.dispatch(&mut session, "vcs.operation.status", json!({})).unwrap()
        });
        let english = zen_i18n::with_language("en", || t!("vcs-push-done", count = 1, branch = "origin/main"));
        assert_eq!(status["message"], english, "{status}");
        let russian = zen_i18n::with_language("ru", || {
            registry.dispatch(&mut session, "vcs.operation.status", json!({})).unwrap()
        });
        assert_ne!(russian["message"], english, "the window reads its own language: {russian}");
    }

    #[test]
    fn push_and_update_go_through_a_remote_with_their_outcome() {
        isolate();
        let remote = tempfile::tempdir().unwrap();
        git(remote.path(), &["init", "-q", "--bare", "-b", "main"]);
        let local = test_repository();
        let root = local.path().canonicalize().unwrap();
        let remote_url = remote.path().to_str().unwrap();
        git(&root, &["remote", "add", "origin", remote_url]);
        let (registry, mut session) = super::super::tests::session(&root);

        let outgoing = registry.dispatch(&mut session, "vcs.push.outgoing", json!({})).unwrap();
        assert_eq!((outgoing["remote"].as_str(), outgoing["target"].as_str()), (Some("origin"), Some("main")));
        assert_eq!(outgoing["newBranch"], true);
        assert_eq!(outgoing["commits"].as_array().unwrap().len(), 1);
        let pushed = registry.dispatch(&mut session, "vcs.remote.push", json!({ "wait": true })).unwrap();
        assert_eq!(pushed["succeeded"], true, "{pushed}");
        let info = registry.dispatch(&mut session, "vcs.repository.info", json!({})).unwrap();
        assert_eq!(info["upstream"], "origin/main");
        assert_eq!(info["ahead"], 0);

        std::fs::write(root.join("b.txt"), "b\n").unwrap();
        git(&root, &["add", "b.txt"]);
        git(&root, &["commit", "-q", "-m", "Second"]);
        let outgoing = registry.dispatch(&mut session, "vcs.push.outgoing", json!({})).unwrap();
        assert_eq!(outgoing["newBranch"], false);
        assert_eq!(outgoing["commits"][0]["subject"], "Second");
        let pushed = registry.dispatch(&mut session, "vcs.remote.push", json!({ "wait": true })).unwrap();
        assert_eq!(pushed["message"], t!("vcs-push-done", count = 1, branch = "origin/main"));

        let other = tempfile::tempdir().unwrap();
        git(other.path(), &["clone", "-q", remote_url, "clone"]);
        let clone = other.path().join("clone");
        std::fs::write(clone.join("c.txt"), "c\n").unwrap();
        git(&clone, &["add", "c.txt"]);
        git(&clone, &["commit", "-q", "-m", "Third"]);
        git(&clone, &["push", "-q", "origin", "main"]);
        let fetched = registry.dispatch(&mut session, "vcs.remote.fetch", json!({ "wait": true })).unwrap();
        assert_eq!(fetched["succeeded"], true, "{fetched}");
        let info = registry.dispatch(&mut session, "vcs.repository.info", json!({})).unwrap();
        assert_eq!(info["behind"], 1);
        let updated = registry.dispatch(&mut session, "vcs.project.update", json!({ "wait": true })).unwrap();
        assert_eq!(updated["message"], t!("vcs-update-done", count = 1), "{updated}");
        assert!(root.join("c.txt").exists());
        let updated = registry.dispatch(&mut session, "vcs.project.update", json!({ "wait": true })).unwrap();
        assert_eq!(updated["message"], t!("vcs-update-up-to-date"));
        let status = registry.dispatch(&mut session, "vcs.operation.status", json!({})).unwrap();
        assert_eq!((status["kind"].as_str(), status["running"].as_bool()), (Some("update"), Some(false)));
    }

    #[test]
    fn update_stopped_on_conflicts_names_the_merge_of_the_upstream() {
        isolate();
        let remote = tempfile::tempdir().unwrap();
        git(remote.path(), &["init", "-q", "--bare", "-b", "main"]);
        let local = test_repository();
        let root = local.path().canonicalize().unwrap();
        git(&root, &["remote", "add", "origin", remote.path().to_str().unwrap()]);
        std::fs::write(root.join("a.txt"), "theirs\n").unwrap();
        git(&root, &["commit", "-q", "-am", "Theirs"]);
        git(&root, &["push", "-q", "-u", "origin", "main"]);
        git(&root, &["reset", "-q", "--hard", "HEAD~1"]);
        std::fs::write(root.join("a.txt"), "ours\n").unwrap();
        git(&root, &["commit", "-q", "-am", "Ours"]);
        let (registry, mut session) = super::super::tests::session(&root);
        let updated = registry.dispatch(&mut session, "vcs.project.update", json!({ "wait": true })).unwrap();
        assert_eq!(updated["succeeded"], false, "{updated}");
        let list = registry.dispatch(&mut session, "vcs.conflict.list", json!({})).unwrap();
        assert_eq!(list["operation"]["title"], t!("vcs-operation-merge", theirs = "origin/main", ours = "main"));
    }

    #[test]
    fn branches_and_tags_are_pushed_and_remote_branches_deleted() {
        isolate();
        let remote = tempfile::tempdir().unwrap();
        git(remote.path(), &["init", "-q", "--bare", "-b", "main"]);
        let local = test_repository();
        let root = local.path().canonicalize().unwrap();
        git(&root, &["remote", "add", "origin", remote.path().to_str().unwrap()]);
        git(&root, &["branch", "topic"]);
        git(&root, &["tag", "v1"]);
        let (registry, mut session) = super::super::tests::session(&root);
        let on_remote = |name: &str| {
            std::process::Command::new("git")
                .current_dir(remote.path())
                .args(["rev-parse", "--verify", "-q", name])
                .output()
                .unwrap()
                .status
                .success()
        };

        let outgoing = registry.dispatch(&mut session, "vcs.push.outgoing", json!({ "branch": "topic" })).unwrap();
        assert_eq!((outgoing["branch"].as_str(), outgoing["newBranch"].as_bool()), (Some("topic"), Some(true)));
        let pushed =
            registry.dispatch(&mut session, "vcs.remote.push", json!({ "branch": "topic", "wait": true })).unwrap();
        assert_eq!(pushed["message"], t!("vcs-push-done", count = 1, branch = "origin/topic"), "{pushed}");
        assert!(on_remote("refs/heads/topic") && !on_remote("refs/heads/main"), "only topic went");
        let list = registry.dispatch(&mut session, "vcs.branch.list", json!({})).unwrap();
        let topic = list["local"].as_array().unwrap().iter().find(|branch| branch["name"] == "topic").unwrap();
        assert_eq!(topic["upstream"], "origin/topic");

        let tag = registry.dispatch(&mut session, "vcs.tag.push", json!({ "name": "v1", "wait": true })).unwrap();
        assert_eq!(tag["message"], t!("vcs-push-tag-done", tag = "v1", remote = "origin"), "{tag}");
        assert!(on_remote("refs/tags/v1"));
        let missing = registry.dispatch(&mut session, "vcs.tag.push", json!({ "name": "v9", "wait": true })).unwrap();
        assert_eq!(missing["succeeded"], false);

        let deleted = registry
            .dispatch(&mut session, "vcs.branch.deleteRemote", json!({ "name": "origin/topic", "wait": true }))
            .unwrap();
        assert_eq!(deleted["message"], t!("vcs-remote-branch-deleted", branch = "origin/topic"), "{deleted}");
        assert!(!on_remote("refs/heads/topic"));
        let list = registry.dispatch(&mut session, "vcs.branch.list", json!({})).unwrap();
        assert_eq!(list["remote"], json!([]), "the remote-tracking branch went too");
        assert!(list["local"].as_array().unwrap().iter().any(|branch| branch["name"] == "topic"), "local stays");
    }

    #[test]
    fn a_failed_push_says_why() {
        let local = test_repository();
        let root = local.path().canonicalize().unwrap();
        let (registry, mut session) = super::super::tests::session(&root);
        assert!(registry.dispatch(&mut session, "vcs.push.outgoing", json!({})).is_err(), "no remote");
        let missing = tempfile::tempdir().unwrap();
        git(&root, &["remote", "add", "origin", missing.path().join("none").to_str().unwrap()]);
        let pushed = registry.dispatch(&mut session, "vcs.remote.push", json!({ "wait": true })).unwrap();
        assert_eq!(pushed["succeeded"], false);
        assert!(!pushed["message"].as_str().unwrap().is_empty());
    }

    #[test]
    fn force_push_is_refused_when_the_remote_branch_moved_after_the_dialog_showed_it() {
        isolate();
        let remote = tempfile::tempdir().unwrap();
        git(remote.path(), &["init", "-q", "--bare", "-b", "main"]);
        let local = test_repository();
        let root = local.path().canonicalize().unwrap();
        git(&root, &["remote", "add", "origin", remote.path().to_str().unwrap()]);
        let (registry, mut session) = super::super::tests::session(&root);
        git(&root, &["push", "-q", "-u", "origin", "main"]);
        std::fs::write(root.join("a.txt"), "amended\n").unwrap();
        git(&root, &["commit", "-q", "-a", "--amend", "-m", "Amended"]);
        let seen = registry.dispatch(&mut session, "vcs.push.outgoing", json!({})).unwrap();
        assert!(seen["remoteTip"].is_string(), "{seen}");

        let other = tempfile::tempdir().unwrap();
        git(other.path(), &["clone", "-q", remote.path().to_str().unwrap(), "clone"]);
        let clone = other.path().join("clone");
        std::fs::write(clone.join("c.txt"), "c\n").unwrap();
        git(&clone, &["add", "c.txt"]);
        git(&clone, &["commit", "-q", "-m", "Theirs"]);
        git(&clone, &["push", "-q", "origin", "main"]);
        git(&root, &["fetch", "-q", "origin"]);

        let params = json!({ "force": true, "remoteTip": seen["remoteTip"], "wait": true });
        let pushed = registry.dispatch(&mut session, "vcs.remote.push", params).unwrap();
        assert_eq!(pushed["succeeded"], false, "{pushed}");
        assert_eq!(pushed["message"], t!("vcs-push-remote-moved", branch = "origin/main"));
    }
}
