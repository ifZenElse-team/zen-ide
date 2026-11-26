use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_vcs::Branch;

use super::background::{self, Step};
use super::history::{self, Done, Operation};
use super::operations::HeadMoved;
use super::{changed, command_error, repository, vcs};

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListBranches)?;
    registry.register(CreateBranch)?;
    registry.register(CheckoutBranch)?;
    registry.register(MergeBranch)?;
    registry.register(RebaseBranch)?;
    registry.register(FastForwardBranch)?;
    registry.register(RenameBranch)?;
    registry.register(SetUpstream)?;
    registry.register(DeleteBranch)?;
    registry.register(DeleteTag)?;
    registry.register(CheckoutRevision)?;
    Ok(())
}

/// A branch, as commands report it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct BranchInfo {
    /// `main`, or `origin/main` for a remote branch.
    pub name: String,
    /// Whether `HEAD` is on it.
    pub current: bool,
    /// The commit it points at, as hex.
    pub commit: String,
    /// The remote branch a local one tracks.
    pub upstream: Option<String>,
    /// Commits to push (on the branch, not on its upstream).
    pub ahead: usize,
    /// Commits to pull (on the upstream, not on the branch).
    pub behind: usize,
}

impl From<&Branch> for BranchInfo {
    fn from(branch: &Branch) -> Self {
        BranchInfo {
            name: branch.name.clone(),
            current: branch.current,
            commit: branch.commit.clone(),
            upstream: branch.upstream.clone(),
            ahead: branch.ahead,
            behind: branch.behind,
        }
    }
}

/// A tag, as commands report it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TagInfo {
    /// `v1.0`.
    pub name: String,
    /// The commit it points at, as hex.
    pub commit: String,
}

const RECENT: usize = 5;

/// The repository's branches.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct BranchList {
    /// The current branch; `None` when `HEAD` is detached.
    pub current: Option<String>,
    /// The local branches checked out last, the current one first (from git's reflog, so that
    /// checkouts made in a terminal count too).
    pub recent: Vec<String>,
    /// Local branches, by name.
    pub local: Vec<BranchInfo>,
    /// Remote-tracking branches, by name.
    pub remote: Vec<BranchInfo>,
    /// Tags, by name.
    pub tags: Vec<TagInfo>,
}

impl BranchList {
    pub fn tracking(&self, remote: &str) -> Option<&BranchInfo> {
        let mut tracking = self.local.iter().filter(|branch| branch.upstream.as_deref() == Some(remote));
        let first = tracking.clone().next();
        tracking.find(|branch| branch.behind > 0 && branch.ahead == 0).or(first)
    }
}

pub struct ListBranches;

impl Command for ListBranches {
    type Params = NoParams;
    type Output = BranchList;
    const ID: &'static str = "vcs.branch.list";
    const TITLE: &'static str = "Branches…";
    const DESCRIPTION: &'static str = "Lists the local and remote-tracking branches, the current one, each local branch's upstream with \
         the commits to push and to pull, the branches checked out recently (`recent`, the current one first) and \
         the tags.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<BranchList, CommandError> {
        let repository = repository(session)?;
        let snapshot = super::fresh_refs(vcs(session));
        let current = snapshot.head.branch.clone();
        let mut recent = repository.recent_branches(RECENT).unwrap_or_default();
        if recent.is_empty() {
            recent.extend(current.clone());
        }
        let tags = repository.tags().unwrap_or_default();
        Ok(BranchList {
            current,
            recent,
            local: snapshot.branches.iter().filter(|branch| !branch.remote).map(BranchInfo::from).collect(),
            remote: snapshot.branches.iter().filter(|branch| branch.remote).map(BranchInfo::from).collect(),
            tags: tags.into_iter().map(|tag| TagInfo { name: tag.name, commit: tag.commit }).collect(),
        })
    }
}

/// A new branch.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateParams {
    /// The new branch's name.
    pub name: String,
    /// Where it starts: a branch, a tag or a commit; `HEAD` when absent.
    #[serde(default)]
    pub start_point: Option<String>,
    /// Check the new branch out (the default, as in the New Branch dialog).
    #[serde(default = "yes")]
    pub checkout: bool,
}

fn yes() -> bool {
    true
}

pub struct CreateBranch;

impl Command for CreateBranch {
    type Params = CreateParams;
    type Output = ();
    const ID: &'static str = "vcs.branch.create";
    const TITLE: &'static str = "New Branch…";
    const DESCRIPTION: &'static str =
        "Creates a branch at HEAD or at `startPoint`, and checks it out unless `checkout` is false.";

    fn run(&self, session: &mut Session, params: CreateParams) -> Result<(), CommandError> {
        let name = params.name.trim();
        if name.is_empty() {
            return Err(CommandError::new(t!("vcs-error-branch-name")));
        }
        let repository = repository(session)?;
        if params.checkout
            && let Some(start) = params.start_point.as_deref().map(str::trim).filter(|start| !start.is_empty())
        {
            crate::hidden_files::guard_revision(session, &repository, start)?;
        }
        let recording = history::begin(session, &repository);
        let result = repository.create_branch(name, params.start_point.as_deref(), params.checkout);
        if result.is_ok() {
            history::finish(session, &repository, recording, Done::new(Operation::BranchCreate, name));
        }
        changed(session);
        result.map_err(|error| CommandError::new(super::message(&error)))
    }
}

/// A branch to act on.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BranchParams {
    /// The branch: `main`, or `origin/main` for a remote one.
    pub name: String,
}

pub struct CheckoutBranch;

impl Command for CheckoutBranch {
    type Params = BranchParams;
    type Output = ();
    const ID: &'static str = "vcs.branch.checkout";
    const TITLE: &'static str = "Checkout";
    const DESCRIPTION: &'static str = "Checks out a local branch; for a remote one (`origin/feature`), checks out the local \
         branch of the same name that tracks it, creating it when needed.";

    fn run(&self, session: &mut Session, params: BranchParams) -> Result<(), CommandError> {
        let repository = repository(session)?;
        crate::hidden_files::guard_revision(session, &repository, params.name.trim())?;
        let recording = history::begin(session, &repository);
        let name = params.name;
        let branch = name.clone();
        background::run(
            session,
            Step::new("checkout", t!("vcs-progress-checkout")),
            move |repository| repository.checkout(&branch),
            move |session, result| {
                if result.is_ok() {
                    history::finish(session, &repository, recording, Done::new(Operation::Checkout, name.trim()));
                }
                changed(session);
                result.map_err(|error| CommandError::new(super::message(&error)))
            },
        )
    }
}

/// A branch, a tag or a commit to merge, and where to.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MergeParams {
    /// What to merge: a branch (`feature`, `origin/feature`), a tag or a commit.
    #[serde(alias = "source")]
    pub name: String,
    /// The local branch to merge it into; the current branch when absent. Another branch is
    /// checked out first (refused while tracked files have uncommitted changes).
    #[serde(default)]
    pub target: Option<String>,
}

pub struct MergeBranch;

impl Command for MergeBranch {
    type Params = MergeParams;
    type Output = ();
    const ID: &'static str = "vcs.branch.merge";
    const TITLE: &'static str = "Merge into Current";
    const DESCRIPTION: &'static str = "Merges the branch `name` (or `source`: local or remote, a tag, a commit) into the current \
         branch, a fast-forward when possible. With `target`, into that local branch instead: it is \
         checked out first, which is refused while tracked files have uncommitted changes (commit or \
         stash them). On conflicts the repository is left as git leaves it (the merge in progress, the \
         files conflicted) and the command fails with the code `conflicts` and what to do.";

    fn run(&self, session: &mut Session, params: MergeParams) -> Result<(), CommandError> {
        let name = super::operations::named(&params.name)?.to_owned();
        let repository = repository(session)?;
        let target = params.target.as_deref().map(str::trim).filter(|target| !target.is_empty()).map(str::to_owned);
        if let Some(target) = &target {
            checks_out(&repository, target)?;
        }
        let into = target.clone().or_else(|| current_label(&repository));
        if target.is_none() {
            crate::hidden_files::guard_revision(session, &repository, &name)?;
        }
        let recording = history::begin(session, &repository);
        let done = Done::new(Operation::Merge, name.as_str()).target(into.unwrap_or_default());
        background::run(
            session,
            Step::new("merge", t!("vcs-progress-merge")),
            move |repository| match &target {
                Some(target) => repository.merge_into(&name, target),
                None => repository.merge(&name),
            },
            move |session, result| {
                match &result {
                    Ok(()) => history::finish(session, &repository, recording, done),
                    Err(zen_vcs::VcsError::Conflicts { .. }) => {
                        history::keep_pending(session, &repository, recording, done);
                    }
                    Err(_) => {}
                }
                changed(session);
                result.map_err(|error| super::reported(session, &error))
            },
        )
    }
}

fn current_label(repository: &zen_vcs::Repository) -> Option<String> {
    repository.head().ok().map(|head| head.label())
}

fn checks_out(repository: &zen_vcs::Repository, branch: &str) -> Result<(), CommandError> {
    let current = repository.head().map_err(|error| command_error(&error))?.branch;
    if current.as_deref() != Some(branch) && repository.has_tracked_changes().map_err(|error| command_error(&error))? {
        return Err(CommandError::new(t!("vcs-error-checkout-changes", branch = branch)));
    }
    Ok(())
}

/// What to rebase onto, and which branch.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RebaseParams {
    /// The branch (`main`, `origin/main`), tag or commit the branch's own commits go on top of.
    #[serde(alias = "target")]
    pub onto: String,
    /// The local branch to rebase; the current branch when absent. Another branch is checked out
    /// first (refused while tracked files have uncommitted changes).
    #[serde(default, alias = "source")]
    pub branch: Option<String>,
}

pub struct RebaseBranch;

impl Command for RebaseBranch {
    type Params = RebaseParams;
    type Output = HeadMoved;
    const ID: &'static str = "vcs.branch.rebase";
    const TITLE: &'static str = "Rebase Current onto";
    const DESCRIPTION: &'static str = "Rebases the current branch onto `onto` (or `target`: a branch, a tag or a commit; git \
         rebase): the branch's own commits are replayed on top of it. With `branch` (or `source`), that \
         local branch is rebased instead: it is checked out first, which is refused while tracked files \
         have uncommitted changes (commit or stash them). Answers with the new HEAD. On conflicts the \
         repository is left as git leaves it (the rebase in progress, the files conflicted) and the \
         command fails with the code `conflicts` and what to do.";

    fn run(&self, session: &mut Session, params: RebaseParams) -> Result<HeadMoved, CommandError> {
        let onto = super::operations::named(&params.onto)?.to_owned();
        let repository = repository(session)?;
        let branch = params.branch.as_deref().map(str::trim).filter(|branch| !branch.is_empty()).map(str::to_owned);
        if let Some(branch) = &branch {
            checks_out(&repository, branch)?;
        }
        let rebased = branch.clone().or_else(|| current_label(&repository));
        if branch.is_none() {
            crate::hidden_files::guard_revision(session, &repository, &onto)?;
        }
        let recording = history::begin(session, &repository);
        let done = Done::new(Operation::Rebase, onto.as_str()).target(rebased.unwrap_or_default());
        background::run(
            session,
            Step::new("rebase", t!("vcs-progress-rebase")),
            move |repository| match &branch {
                Some(branch) => repository.rebase_branch(branch, &onto),
                None => repository.rebase(&onto),
            },
            move |session, result| {
                match &result {
                    Ok(_) => history::finish(session, &repository, recording, done),
                    Err(zen_vcs::VcsError::Conflicts { .. }) => {
                        history::keep_pending(session, &repository, recording, done);
                    }
                    Err(_) => {}
                }
                super::operations::applied(session, &repository, result)
            },
        )
    }
}

fn branch_info(repository: &zen_vcs::Repository, name: &str) -> Result<BranchInfo, CommandError> {
    let branches = repository.branches().map_err(|error| command_error(&error))?;
    branches
        .iter()
        .find(|branch| !branch.remote && branch.name == name)
        .map(BranchInfo::from)
        .ok_or_else(|| CommandError::new(t!("vcs-error-no-branch", branch = name)))
}

/// A local branch to fast-forward.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FastForwardParams {
    /// The local branch.
    pub name: String,
    /// Where to: a branch (`origin/main`), a tag or a commit; the branch's upstream when absent.
    #[serde(default)]
    pub onto: Option<String>,
}

pub struct FastForwardBranch;

impl Command for FastForwardBranch {
    type Params = FastForwardParams;
    type Output = BranchInfo;
    const ID: &'static str = "vcs.branch.fastForward";
    const TITLE: &'static str = "Fast-forward";
    const DESCRIPTION: &'static str = "Fast-forwards a local branch to its upstream as last fetched (or to `onto`), without \
         talking to the remote: the current branch with its files, another one by moving it. A branch \
         with commits of its own is refused (merge or rebase instead). Answers with the branch.";

    fn run(&self, session: &mut Session, params: FastForwardParams) -> Result<BranchInfo, CommandError> {
        let name = super::operations::named(&params.name)?;
        let repository = repository(session)?;
        if current_label(&repository).as_deref() == Some(name) {
            let onto = params.onto.as_deref().map(str::trim).filter(|onto| !onto.is_empty());
            let target = onto.map_or_else(|| format!("{name}@{{upstream}}"), str::to_owned);
            crate::hidden_files::guard_revision(session, &repository, &target)?;
        }
        let recording = history::begin(session, &repository);
        let result = repository.fast_forward(name, params.onto.as_deref());
        if result.is_ok() {
            history::finish(session, &repository, recording, Done::new(Operation::FastForward, name));
        }
        changed(session);
        result.map_err(|error| command_error(&error))?;
        branch_info(&repository, name)
    }
}

/// A local branch to rename.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenameParams {
    /// The local branch.
    pub name: String,
    /// Its new name.
    pub new_name: String,
}

pub struct RenameBranch;

impl Command for RenameBranch {
    type Params = RenameParams;
    type Output = BranchInfo;
    const ID: &'static str = "vcs.branch.rename";
    const TITLE: &'static str = "Rename Branch";
    const DESCRIPTION: &'static str = "Renames a local branch (the current one too); its upstream and history go with it. A name \
         that is taken or not valid is refused. Answers with the renamed branch.";

    fn run(&self, session: &mut Session, params: RenameParams) -> Result<BranchInfo, CommandError> {
        let name = super::operations::named(&params.name)?;
        let new_name = params.new_name.trim();
        if new_name.is_empty() {
            return Err(CommandError::new(t!("vcs-error-branch-name")));
        }
        let repository = repository(session)?;
        let result = repository.rename_branch(name, new_name);
        changed(session);
        result.map_err(|error| command_error(&error))?;
        vcs(session).log_view.renamed_branch(name, new_name);
        branch_info(&repository, new_name)
    }
}

/// The upstream to give a local branch.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamParams {
    /// The local branch.
    pub name: String,
    /// The remote branch it tracks from now on (`origin/main`); none when absent.
    #[serde(default)]
    pub upstream: Option<String>,
}

pub struct SetUpstream;

impl Command for SetUpstream {
    type Params = UpstreamParams;
    type Output = BranchInfo;
    const ID: &'static str = "vcs.branch.setUpstream";
    const TITLE: &'static str = "Set Upstream";
    const DESCRIPTION: &'static str = "Makes a local branch track a remote branch (`upstream: origin/main`): pull and push go \
         there and the ahead and behind counts compare with it. Without `upstream` the branch tracks \
         nothing. Answers with the branch.";

    fn run(&self, session: &mut Session, params: UpstreamParams) -> Result<BranchInfo, CommandError> {
        let name = super::operations::named(&params.name)?;
        let repository = repository(session)?;
        let result = repository.set_upstream(name, params.upstream.as_deref());
        changed(session);
        result.map_err(|error| command_error(&error))?;
        branch_info(&repository, name)
    }
}

/// A tag to delete.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TagNameParams {
    /// The tag: `v1.0`.
    pub name: String,
}

pub struct DeleteTag;

impl Command for DeleteTag {
    type Params = TagNameParams;
    type Output = ();
    const ID: &'static str = "vcs.tag.delete";
    const TITLE: &'static str = "Delete Tag";
    const DESCRIPTION: &'static str =
        "Deletes a tag in the local repository; a remote that has it keeps it (vcs.tag.push sends tags).";

    fn run(&self, session: &mut Session, params: TagNameParams) -> Result<(), CommandError> {
        let name = params.name.trim();
        if name.is_empty() {
            return Err(CommandError::new(t!("vcs-error-tag-name")));
        }
        let repository = repository(session)?;
        let result = repository.delete_tag(name);
        changed(session);
        result.map_err(|error| command_error(&error))
    }
}

/// A branch to delete.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteParams {
    /// The local branch.
    pub name: String,
    /// Delete it even if it is not merged.
    #[serde(default)]
    pub force: bool,
}

pub struct DeleteBranch;

impl Command for DeleteBranch {
    type Params = DeleteParams;
    type Output = ();
    const ID: &'static str = "vcs.branch.delete";
    const TITLE: &'static str = "Delete";
    const DESCRIPTION: &'static str = "Deletes a local branch; one that is not merged is deleted only with `force`. The current branch \
         cannot be deleted.";

    fn run(&self, session: &mut Session, params: DeleteParams) -> Result<(), CommandError> {
        let repository = repository(session)?;
        let recording = history::begin(session, &repository);
        let result = repository.delete_branch(&params.name, params.force);
        if result.is_ok() {
            history::finish(session, &repository, recording, Done::new(Operation::BranchDelete, params.name.trim()));
        }
        changed(session);
        result.map_err(|error| CommandError::new(super::message(&error)))
    }
}

/// A tag or a revision to check out.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RevisionParams {
    /// A tag (`v1.0`), a commit id, or any revision git understands (`HEAD~2`, `origin/main`).
    pub revision: String,
}

pub struct CheckoutRevision;

impl Command for CheckoutRevision {
    type Params = RevisionParams;
    type Output = ();
    const ID: &'static str = "vcs.revision.checkout";
    const TITLE: &'static str = "Checkout Tag or Revision…";
    const DESCRIPTION: &'static str = "Checks out a tag, a commit or another revision with a detached HEAD: no branch is \
         checked out afterwards.";

    fn run(&self, session: &mut Session, params: RevisionParams) -> Result<(), CommandError> {
        let revision = params.revision.trim().to_owned();
        if revision.is_empty() {
            return Err(CommandError::new(t!("vcs-error-revision")));
        }
        let repository = repository(session)?;
        crate::hidden_files::guard_revision(session, &repository, &revision)?;
        let recording = history::begin(session, &repository);
        let target = revision.clone();
        background::run(
            session,
            Step::new("checkout", t!("vcs-progress-checkout")),
            move |repository| repository.checkout_revision(&target),
            move |session, result| {
                if result.is_ok() {
                    history::finish(session, &repository, recording, Done::new(Operation::Checkout, revision));
                }
                changed(session);
                result.map_err(|error| CommandError::new(super::message(&error)))
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::testing::*;
    use super::*;

    #[test]
    fn branches_are_created_listed_checked_out_merged_and_deleted() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        let (registry, mut session) = super::super::tests::session(&root);
        let wait = |registry: &Registry, session: &mut Session| {
            registry.dispatch(session, "vcs.status.refresh", json!({ "wait": true })).unwrap();
        };
        wait(&registry, &mut session);
        let list = registry.dispatch(&mut session, "vcs.branch.list", json!({})).unwrap();
        assert_eq!(list["current"], "main");
        assert_eq!(list["local"].as_array().unwrap().len(), 1);

        registry.dispatch(&mut session, "vcs.branch.create", json!({ "name": "feature" })).unwrap();
        wait(&registry, &mut session);
        let info = registry.dispatch(&mut session, "vcs.repository.info", json!({})).unwrap();
        assert_eq!(info["branch"], "feature", "a new branch is checked out");
        std::fs::write(root.join("b.txt"), "b\n").unwrap();
        git(&root, &["add", "b.txt"]);
        git(&root, &["commit", "-q", "-m", "Second"]);

        registry.dispatch(&mut session, "vcs.branch.checkout", json!({ "name": "main" })).unwrap();
        wait(&registry, &mut session);
        assert!(!root.join("b.txt").exists(), "main has no b.txt");
        registry.dispatch(&mut session, "vcs.branch.merge", json!({ "name": "feature" })).unwrap();
        assert!(root.join("b.txt").exists(), "merged");
        registry.dispatch(&mut session, "vcs.branch.create", json!({ "name": "other", "checkout": false })).unwrap();
        wait(&registry, &mut session);
        let list = registry.dispatch(&mut session, "vcs.branch.list", json!({})).unwrap();
        let names: Vec<&str> = list["local"].as_array().unwrap().iter().map(|b| b["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["feature", "main", "other"]);
        assert_eq!(list["current"], "main");

        registry.dispatch(&mut session, "vcs.branch.delete", json!({ "name": "other" })).unwrap();
        assert!(registry.dispatch(&mut session, "vcs.branch.delete", json!({ "name": "main" })).is_err());
        assert!(registry.dispatch(&mut session, "vcs.branch.create", json!({ "name": " " })).is_err());
        wait(&registry, &mut session);
        let list = registry.dispatch(&mut session, "vcs.branch.list", json!({})).unwrap();
        assert_eq!(list["local"].as_array().unwrap().len(), 2);
        assert_eq!(list["recent"], json!(["main", "feature"]), "checked out last, the current one first");
    }

    fn with_origin() -> (tempfile::TempDir, tempfile::TempDir, std::path::PathBuf) {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        git(&root, &["switch", "-q", "-c", "feature"]);
        std::fs::write(root.join("f.txt"), "feature\n").unwrap();
        git(&root, &["add", "f.txt"]);
        git(&root, &["commit", "-q", "-m", "Feature"]);
        git(&root, &["switch", "-q", "main"]);
        std::fs::write(root.join("a.txt"), "one\ntwo\nthree\nfour\n").unwrap();
        git(&root, &["commit", "-q", "-am", "Second"]);
        let remote = tempfile::tempdir().unwrap();
        git(remote.path(), &["init", "-q", "--bare", "-b", "main"]);
        git(&root, &["remote", "add", "origin", remote.path().to_str().unwrap()]);
        git(&root, &["push", "-q", "-u", "origin", "main", "feature"]);
        (dir, remote, root)
    }

    #[test]
    fn branches_are_renamed_given_upstreams_and_fast_forwarded() {
        let (_dir, _remote, root) = with_origin();
        let (registry, mut session) = super::super::tests::session(&root);
        let run = |session: &mut Session, command: &str, params: serde_json::Value| {
            registry.dispatch(session, command, params)
        };
        let s = &mut session;
        run(s, "vcs.branch.favorite", json!({ "name": "feature", "favorite": true })).unwrap();
        let renamed = run(s, "vcs.branch.rename", json!({ "name": "feature", "newName": "topic" })).unwrap();
        assert_eq!((renamed["name"].as_str(), renamed["upstream"].as_str()), (Some("topic"), Some("origin/feature")));
        let favorites = run(s, "vcs.branch.favorites", json!({})).unwrap();
        assert!(favorites.as_array().unwrap().contains(&json!("topic")), "the favourite follows: {favorites}");
        assert!(run(s, "vcs.branch.rename", json!({ "name": "topic", "newName": " " })).is_err());
        assert!(run(s, "vcs.branch.rename", json!({ "name": "topic", "newName": "main" })).is_err(), "taken");

        let set = run(s, "vcs.branch.setUpstream", json!({ "name": "topic", "upstream": "origin/main" })).unwrap();
        assert_eq!(set["upstream"], "origin/main");
        assert_eq!(set["behind"], 1, "compared with the new upstream");
        let unset = run(s, "vcs.branch.setUpstream", json!({ "name": "topic" })).unwrap();
        assert_eq!(unset["upstream"], json!(null));

        git(&root, &["branch", "-q", "--track", "old", "origin/main"]);
        git(&root, &["update-ref", "refs/heads/old", "main~1"]);
        let moved = run(s, "vcs.branch.fastForward", json!({ "name": "old" })).unwrap();
        assert_eq!(moved["commit"], git(&root, &["rev-parse", "main"]).trim());
        assert_eq!((moved["behind"].as_u64(), moved["current"].as_bool()), (Some(0), Some(false)));
        let refused = run(s, "vcs.branch.fastForward", json!({ "name": "topic", "onto": "main" })).unwrap_err();
        assert!(refused.to_string().contains(&t!("vcs-error-not-fast-forward", branch = "topic", target = "main")));
        assert!(run(s, "vcs.branch.fastForward", json!({ "name": "topic" })).is_err(), "no upstream");
    }

    #[test]
    fn merge_and_rebase_onto_answer_or_stop_on_conflicts() {
        let (_dir, _remote, root) = with_origin();
        let (registry, mut session) = super::super::tests::session(&root);
        let s = &mut session;
        let rebased = registry.dispatch(s, "vcs.branch.rebase", json!({ "onto": "feature" })).unwrap();
        assert_eq!(rebased["head"], git(&root, &["rev-parse", "HEAD"]).trim());
        assert_eq!(rebased["branch"], "main");
        assert!(root.join("f.txt").exists(), "main is on top of feature now");
        assert!(registry.dispatch(s, "vcs.branch.rebase", json!({ "onto": "" })).is_err());

        git(&root, &["switch", "-q", "-c", "other", "main~2"]);
        std::fs::write(root.join("a.txt"), "one\ntwo\nTHREE\n").unwrap();
        git(&root, &["commit", "-q", "-am", "Other"]);
        git(&root, &["switch", "-q", "main"]);
        for (command, params, abort) in [
            ("vcs.branch.merge", json!({ "name": "other" }), "git merge --abort"),
            ("vcs.branch.rebase", json!({ "onto": "other" }), "git rebase --abort"),
        ] {
            let failed = registry.dispatch(s, command, params).unwrap_err();
            let zen_core::DispatchError::Failed { error, .. } = failed else { panic!("{failed:?}") };
            assert_eq!(error.code.as_deref(), Some(super::super::CONFLICTS), "{command}");
            assert!(error.message.contains("a.txt") && error.message.contains("Resolve…"), "{}", error.message);
            let told = s.state_mut::<super::super::workspace::GitWorkspace>().take_conflicts();
            assert_eq!(told.as_deref(), Some(error.message.as_str()), "the window tells the user too");
            let abort: Vec<&str> = abort.split(' ').skip(1).collect();
            git(&root, &abort);
        }
    }

    #[test]
    fn merge_into_and_rebase_of_another_branch_check_it_out_first() {
        let (_dir, _remote, root) = with_origin();
        let (registry, mut session) = super::super::tests::session(&root);
        let s = &mut session;
        let rev = |revision: &str| git(&root, &["rev-parse", revision]).trim().to_owned();
        let current = |root: &std::path::Path| git(root, &["branch", "--show-current"]).trim().to_owned();
        let main = rev("main");
        registry.dispatch(s, "vcs.branch.merge", json!({ "source": "main", "target": "feature" })).unwrap();
        assert_eq!(current(&root), "feature");
        assert_eq!(git(&root, &["merge-base", "--is-ancestor", &main, "feature"]), "");
        assert_eq!(rev("main"), main, "main stays");

        git(&root, &["switch", "-q", "-c", "topic", "main~1"]);
        std::fs::write(root.join("t.txt"), "topic\n").unwrap();
        git(&root, &["add", "t.txt"]);
        git(&root, &["commit", "-q", "-m", "Topic"]);
        git(&root, &["switch", "-q", "main"]);
        let rebased =
            registry.dispatch(s, "vcs.branch.rebase", json!({ "source": "topic", "target": "main" })).unwrap();
        assert_eq!(rebased["branch"], "topic");
        assert_eq!(rebased["head"], rev("topic"));
        assert_eq!(rev("topic~1"), main, "topic's own commit is on top of main");

        std::fs::write(root.join("a.txt"), "edited\n").unwrap();
        for (command, params, branch) in [
            ("vcs.branch.merge", json!({ "name": "main", "target": "feature" }), "feature"),
            ("vcs.branch.rebase", json!({ "onto": "feature", "branch": "main" }), "main"),
        ] {
            let failed = registry.dispatch(s, command, params).unwrap_err();
            let zen_core::DispatchError::Failed { error, .. } = failed else { panic!("{failed:?}") };
            assert_eq!(error.message, t!("vcs-error-checkout-changes", branch = branch), "{command}");
            assert_eq!(current(&root), "topic", "{command}: nothing was checked out");
        }
        let into_current = registry.dispatch(s, "vcs.branch.merge", json!({ "name": "main", "target": "topic" }));
        assert!(into_current.is_ok(), "{into_current:?}");
        git(&root, &["checkout", "-q", "--", "a.txt"]);
        assert!(registry.dispatch(s, "vcs.branch.merge", json!({ "name": "main", "target": "origin/main" })).is_err());
    }

    #[test]
    fn tags_are_deleted_here_only() {
        let (_dir, remote, root) = with_origin();
        git(&root, &["tag", "v1"]);
        git(&root, &["push", "-q", "origin", "v1"]);
        let (registry, mut session) = super::super::tests::session(&root);
        registry.dispatch(&mut session, "vcs.tag.delete", json!({ "name": "v1" })).unwrap();
        let list = registry.dispatch(&mut session, "vcs.branch.list", json!({})).unwrap();
        assert_eq!(list["tags"], json!([]));
        git(remote.path(), &["rev-parse", "--verify", "-q", "refs/tags/v1"]);
        assert!(registry.dispatch(&mut session, "vcs.tag.delete", json!({ "name": "v1" })).is_err());
        assert!(registry.dispatch(&mut session, "vcs.tag.delete", json!({ "name": "" })).is_err());
    }

    #[test]
    fn tags_and_revisions_are_checked_out_detached() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        git(&root, &["tag", "v1"]);
        let (registry, mut session) = super::super::tests::session(&root);
        let list = registry.dispatch(&mut session, "vcs.branch.list", json!({})).unwrap();
        assert_eq!(list["recent"], json!(["main"]), "no checkout yet: the current branch");
        assert_eq!(list["tags"][0]["name"], "v1");
        let commit = list["local"][0]["commit"].clone();
        assert_eq!(list["tags"][0]["commit"], commit);

        assert!(registry.dispatch(&mut session, "vcs.revision.checkout", json!({ "revision": " " })).is_err());
        assert!(registry.dispatch(&mut session, "vcs.revision.checkout", json!({ "revision": "nope" })).is_err());
        registry.dispatch(&mut session, "vcs.revision.checkout", json!({ "revision": "v1" })).unwrap();
        registry.dispatch(&mut session, "vcs.status.refresh", json!({ "wait": true })).unwrap();
        let info = registry.dispatch(&mut session, "vcs.repository.info", json!({})).unwrap();
        assert_eq!(info["branch"], json!(null), "HEAD is detached");
        assert_eq!(info["commit"], commit);
    }
}
