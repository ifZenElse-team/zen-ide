use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};

use super::remote::{Fetch, OperationInfo, UpdateParams, UpdateProject, WaitParams};
use super::repository;
use super::workspace::GitWorkspace;

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(Pull)?;
    registry.register(PullDefault)?;
    Ok(())
}

/// What Pull does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum PullMode {
    /// Fetch every remote; the branches stay where they are.
    FetchAll,
    /// Pull the upstream: a fast-forward when the branch has no commits of its own, a merge
    /// otherwise.
    #[default]
    FastForward,
    /// Pull the upstream only as a fast-forward; refused when the branch has commits of its own.
    FastForwardOnly,
    /// Pull the upstream, rebasing the branch's own commits onto it.
    Rebase,
}

impl PullMode {
    pub const ALL: [PullMode; 4] =
        [PullMode::FetchAll, PullMode::FastForward, PullMode::FastForwardOnly, PullMode::Rebase];

    pub fn message_id(self) -> &'static str {
        match self {
            PullMode::FetchAll => "vcs-workspace-fetch-all",
            PullMode::FastForward => "vcs-workspace-pull-fast-forward",
            PullMode::FastForwardOnly => "vcs-workspace-pull-fast-forward-only",
            PullMode::Rebase => "vcs-workspace-pull-rebase",
        }
    }
}

/// A pull to run.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PullParams {
    /// How; without it, the default (vcs.workspace.pullDefault), as the Pull button does.
    pub mode: Option<PullMode>,
    /// Wait until it is over and answer with its outcome; otherwise it runs in the background and
    /// vcs.operation.status follows it.
    pub wait: bool,
}

pub struct Pull;

impl Command for Pull {
    type Params = PullParams;
    type Output = OperationInfo;
    const ID: &'static str = "vcs.workspace.pull";
    const TITLE: &'static str = "Pull";
    const DESCRIPTION: &'static str = "Pulls as the Git workspace's Pull button (the default mode, vcs.workspace.pullDefault) or \
         an item of its menu (`mode`) does: `fetchAll` fetches every remote (vcs.remote.fetch), \
         `fastForward` pulls the upstream fast-forwarding when it can and merging otherwise, \
         `fastForwardOnly` only fast-forwards (refused when the branch has commits of its own), \
         `rebase` rebases the branch's commits onto the upstream (vcs.project.update). In the \
         background unless `wait` is set; vcs.operation.status reports the progress and the outcome.";

    fn run(&self, session: &mut Session, params: PullParams) -> Result<OperationInfo, CommandError> {
        repository(session)?;
        let workspace = session.state_mut::<GitWorkspace>();
        workspace.pull_menu = false;
        let mode = params.mode.unwrap_or(workspace.pull_mode);
        let wait = params.wait;
        let update = |rebase, fast_forward_only| UpdateParams { rebase, fast_forward_only, wait };
        match mode {
            PullMode::FetchAll => Fetch.run(session, WaitParams { wait }),
            PullMode::FastForward => UpdateProject.run(session, update(false, false)),
            PullMode::FastForwardOnly => UpdateProject.run(session, update(false, true)),
            PullMode::Rebase => UpdateProject.run(session, update(true, false)),
        }
    }
}

/// The default to choose, or nothing to read it.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PullDefaultParams {
    /// What the Pull button does from now on.
    pub mode: Option<PullMode>,
}

/// What the Pull button does.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PullDefaultInfo {
    /// The mode it runs.
    pub mode: PullMode,
}

pub struct PullDefault;

impl Command for PullDefault {
    type Params = PullDefaultParams;
    type Output = PullDefaultInfo;
    const ID: &'static str = "vcs.workspace.pullDefault";
    const TITLE: &'static str = "Set Default Pull";
    const DESCRIPTION: &'static str = "Chooses what the Git workspace's Pull button (and ⌘T in the tab) does, as the marks in \
         Pull's menu do: `fetchAll`, `fastForward` (the default: fast-forward if possible), \
         `fastForwardOnly` or `rebase`; the window remembers it per repository. Without `mode`, \
         answers with the one in use.";

    fn run(&self, session: &mut Session, params: PullDefaultParams) -> Result<PullDefaultInfo, CommandError> {
        let workspace = session.state_mut::<GitWorkspace>();
        if let Some(mode) = params.mode {
            workspace.pull_mode = mode;
        }
        Ok(PullDefaultInfo { mode: workspace.pull_mode })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::testing::*;

    fn behind() -> (tempfile::TempDir, tempfile::TempDir, tempfile::TempDir, std::path::PathBuf) {
        isolate();
        let remote = tempfile::tempdir().unwrap();
        git(remote.path(), &["init", "-q", "--bare", "-b", "main"]);
        let local = test_repository();
        let root = local.path().canonicalize().unwrap();
        let url = remote.path().to_str().unwrap().to_owned();
        git(&root, &["remote", "add", "origin", &url]);
        git(&root, &["push", "-q", "-u", "origin", "main"]);
        let other = tempfile::tempdir().unwrap();
        git(other.path(), &["clone", "-q", &url, "clone"]);
        let clone = other.path().join("clone");
        std::fs::write(clone.join("up.txt"), "up\n").unwrap();
        git(&clone, &["add", "up.txt"]);
        git(&clone, &["commit", "-q", "-m", "Upstream"]);
        git(&clone, &["push", "-q", "origin", "main"]);
        (remote, local, other, root)
    }

    fn local_commit(root: &std::path::Path) {
        std::fs::write(root.join("mine.txt"), "mine\n").unwrap();
        git(root, &["add", "mine.txt"]);
        git(root, &["commit", "-q", "-m", "Mine"]);
    }

    fn parents(root: &std::path::Path) -> usize {
        git(root, &["rev-list", "--parents", "-n", "1", "HEAD"]).split_whitespace().count() - 1
    }

    #[test]
    fn the_default_mode_is_remembered_and_the_button_runs_it() {
        let (_remote, _local, _other, root) = behind();
        let (registry, mut session) = super::super::tests::session(&root);
        let read = registry.dispatch(&mut session, "vcs.workspace.pullDefault", json!({})).unwrap();
        assert_eq!(read["mode"], "fastForward", "fast-forward if possible, to begin with");
        let set = registry.dispatch(&mut session, "vcs.workspace.pullDefault", json!({ "mode": "fetchAll" })).unwrap();
        assert_eq!(set["mode"], "fetchAll");
        let state = registry.dispatch(&mut session, "vcs.workspace.state", json!({})).unwrap();
        assert_eq!(state["pullMode"], "fetchAll");
        assert!(registry.dispatch(&mut session, "vcs.workspace.pullDefault", json!({ "mode": "squash" })).is_err());
        registry.dispatch(&mut session, "vcs.workspace.pullMenu", json!({})).unwrap();
        let fetched = registry.dispatch(&mut session, "vcs.workspace.pull", json!({ "wait": true })).unwrap();
        assert_eq!(
            (fetched["kind"].as_str(), fetched["succeeded"].as_bool()),
            (Some("fetch"), Some(true)),
            "{fetched}"
        );
        assert!(!root.join("up.txt").exists());
        let info = registry.dispatch(&mut session, "vcs.repository.info", json!({})).unwrap();
        assert_eq!(info["behind"], 1);
        let state = registry.dispatch(&mut session, "vcs.workspace.state", json!({})).unwrap();
        assert_eq!(state["pullMenu"], false, "pulling closes the menu");
        let pulled = registry
            .dispatch(&mut session, "vcs.workspace.pull", json!({ "mode": "fastForward", "wait": true }))
            .unwrap();
        assert_eq!(pulled["succeeded"], true, "{pulled}");
        assert!(root.join("up.txt").exists());
        assert_eq!(parents(&root), 1, "a fast-forward: no merge commit");
        let read = registry.dispatch(&mut session, "vcs.workspace.pullDefault", json!({})).unwrap();
        assert_eq!(read["mode"], "fetchAll");
    }

    #[test]
    fn fast_forward_if_possible_merges_a_branch_of_its_own() {
        let (_remote, _local, _other, root) = behind();
        local_commit(&root);
        let (registry, mut session) = super::super::tests::session(&root);
        let pulled = registry
            .dispatch(&mut session, "vcs.workspace.pull", json!({ "mode": "fastForward", "wait": true }))
            .unwrap();
        assert_eq!(pulled["succeeded"], true, "{pulled}");
        assert_eq!(parents(&root), 2, "a merge commit");
        assert!(root.join("up.txt").exists() && root.join("mine.txt").exists());
    }

    #[test]
    fn fast_forward_only_refuses_a_branch_of_its_own_and_leaves_it() {
        let (_remote, _local, _other, root) = behind();
        local_commit(&root);
        let before = git(&root, &["rev-parse", "HEAD"]);
        let (registry, mut session) = super::super::tests::session(&root);
        let pulled = registry
            .dispatch(&mut session, "vcs.workspace.pull", json!({ "mode": "fastForwardOnly", "wait": true }))
            .unwrap();
        assert_eq!(pulled["succeeded"], false, "{pulled}");
        assert_eq!(git(&root, &["rev-parse", "HEAD"]), before);
        assert!(!root.join("up.txt").exists());
        git(&root, &["reset", "-q", "--hard", "HEAD~1"]);
        let pulled = registry
            .dispatch(&mut session, "vcs.project.update", json!({ "fastForwardOnly": true, "wait": true }))
            .unwrap();
        assert_eq!(pulled["succeeded"], true, "{pulled}");
        assert!(root.join("up.txt").exists());
        assert!(
            registry
                .dispatch(&mut session, "vcs.project.update", json!({ "fastForwardOnly": true, "rebase": true }))
                .is_err(),
            "one mode at a time"
        );
    }

    #[test]
    fn rebase_puts_the_branchs_commits_on_the_upstream() {
        let (_remote, _local, _other, root) = behind();
        local_commit(&root);
        let (registry, mut session) = super::super::tests::session(&root);
        registry.dispatch(&mut session, "vcs.workspace.pullDefault", json!({ "mode": "rebase" })).unwrap();
        let pulled = registry.dispatch(&mut session, "vcs.workspace.pull", json!({ "wait": true })).unwrap();
        assert_eq!(pulled["succeeded"], true, "{pulled}");
        assert_eq!(parents(&root), 1, "no merge commit");
        assert_eq!(git(&root, &["log", "-1", "--format=%s", "HEAD~1"]).trim(), "Upstream");
    }
}
