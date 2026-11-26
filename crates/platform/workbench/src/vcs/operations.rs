use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;

use super::background::{self, Step};
use super::branches::TagInfo;
use super::history::{self, Done, MoveMode, Operation};
use super::{changed, command_error, repository};

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(CherryPick)?;
    registry.register(Revert)?;
    registry.register(ResetBranch)?;
    registry.register(CreateTag)?;
    registry.register(CopyCommit)?;
    Ok(())
}

/// Where `HEAD` is after an operation.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct HeadMoved {
    /// The commit `HEAD` is at now: the new commit of a cherry-pick or a revert, the target of a
    /// reset.
    pub head: String,
    /// The branch `HEAD` is on; `None` when it is detached.
    pub branch: Option<String>,
}

fn head_moved(repository: &zen_vcs::Repository, head: String) -> HeadMoved {
    HeadMoved { head, branch: repository.head().ok().and_then(|head| head.branch) }
}

/// A commit to apply to the current branch.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplyParams {
    /// The commit: its id (or the start of it), a branch, a tag.
    pub id: String,
    /// For a merge commit, the parent (from 1) its changes are taken against; the first parent
    /// when absent. Not used for other commits.
    #[serde(default)]
    pub mainline: Option<u32>,
}

pub(super) fn named(id: &str) -> Result<&str, CommandError> {
    let id = id.trim();
    if id.is_empty() { Err(CommandError::new(t!("vcs-error-revision"))) } else { Ok(id) }
}

pub struct CherryPick;

impl Command for CherryPick {
    type Params = ApplyParams;
    type Output = HeadMoved;
    const ID: &'static str = "vcs.commit.cherryPick";
    const TITLE: &'static str = "Cherry-Pick";
    const DESCRIPTION: &'static str = "Applies a commit's changes to the current branch as a new commit (git cherry-pick) and \
         answers with the new HEAD; a merge commit against its first parent unless `mainline` says \
         otherwise. A commit already in the current branch's history is refused. On conflicts the \
         repository is left as git leaves it (the cherry-pick in progress, the files conflicted) and \
         the command fails with the code `conflicts` and what to do.";

    fn run(&self, session: &mut Session, params: ApplyParams) -> Result<HeadMoved, CommandError> {
        let id = named(&params.id)?.to_owned();
        let repository = repository(session)?;
        if repository.is_ancestor(&id, "HEAD").unwrap_or(false) {
            return Err(CommandError::new(t!("vcs-error-already-on-branch")));
        }
        let subject = subject_of(&repository, &id);
        crate::hidden_files::guard_commit(session, &repository, &id)?;
        let recording = history::begin(session, &repository);
        let mainline = params.mainline;
        background::run(
            session,
            Step::new("cherryPick", t!("vcs-progress-cherry-pick")),
            move |repository| repository.cherry_pick(&id, mainline),
            move |session, result| {
                recorded(session, &repository, &result, recording, Done::new(Operation::CherryPick, subject));
                applied(session, &repository, result)
            },
        )
    }
}

pub struct Revert;

impl Command for Revert {
    type Params = ApplyParams;
    type Output = HeadMoved;
    const ID: &'static str = "vcs.commit.revert";
    const TITLE: &'static str = "Revert Commit";
    const DESCRIPTION: &'static str = "Makes a new commit on the current branch that undoes a commit's changes (git revert, \
         with git's message) and answers with the new HEAD; a merge commit is undone against its \
         first parent unless `mainline` says otherwise. On conflicts the repository is left as git \
         leaves it (the revert in progress) and the command fails with the code `conflicts` and what \
         to do.";

    fn run(&self, session: &mut Session, params: ApplyParams) -> Result<HeadMoved, CommandError> {
        let id = named(&params.id)?;
        let repository = repository(session)?;
        let subject = subject_of(&repository, id);
        crate::hidden_files::guard_commit(session, &repository, id)?;
        let recording = history::begin(session, &repository);
        let (id, mainline) = (id.to_owned(), params.mainline);
        background::run(
            session,
            Step::new("revert", t!("vcs-progress-revert")),
            move |repository| repository.revert(&id, mainline),
            move |session, result| {
                recorded(session, &repository, &result, recording, Done::new(Operation::Revert, subject));
                applied(session, &repository, result)
            },
        )
    }
}

fn recorded(
    session: &mut Session,
    repository: &zen_vcs::Repository,
    result: &Result<String, zen_vcs::VcsError>,
    recording: Option<history::Recording>,
    done: Done,
) {
    match result {
        Ok(_) => history::finish(session, repository, recording, done),
        Err(zen_vcs::VcsError::Conflicts { .. }) => history::keep_pending(session, repository, recording, done),
        Err(_) => {}
    }
}

fn subject_of(repository: &zen_vcs::Repository, id: &str) -> String {
    repository.commit_details(id).map_or_else(|_| id.to_owned(), |(commit, _)| commit.subject)
}

pub(super) fn applied(
    session: &mut Session,
    repository: &zen_vcs::Repository,
    result: Result<String, zen_vcs::VcsError>,
) -> Result<HeadMoved, CommandError> {
    changed(session);
    match result {
        Ok(head) => Ok(head_moved(repository, head)),
        Err(error) => Err(super::reported(session, &error)),
    }
}

/// How far a reset goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ResetMode {
    /// Keep the index and the files: the differences are staged.
    Soft,
    /// Keep the files, reset the index: the differences are unstaged changes.
    #[default]
    Mixed,
    /// Reset the index and the files too: every uncommitted change is lost.
    Hard,
}

impl From<ResetMode> for zen_vcs::ResetMode {
    fn from(mode: ResetMode) -> Self {
        match mode {
            ResetMode::Soft => zen_vcs::ResetMode::Soft,
            ResetMode::Mixed => zen_vcs::ResetMode::Mixed,
            ResetMode::Hard => zen_vcs::ResetMode::Hard,
        }
    }
}

/// Where to reset the current branch to, and how.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResetParams {
    /// The commit: its id (or the start of it), a branch, a tag, `HEAD~2`.
    pub revision: String,
    /// `soft`, `mixed` (the default) or `hard`.
    #[serde(default)]
    pub mode: ResetMode,
}

pub struct ResetBranch;

impl Command for ResetBranch {
    type Params = ResetParams;
    type Output = HeadMoved;
    const ID: &'static str = "vcs.branch.reset";
    const TITLE: &'static str = "Reset Current Branch to Here";
    const DESCRIPTION: &'static str = "Moves the current branch (or a detached HEAD) to a commit (git reset): `soft` keeps the \
         differences staged, `mixed` (the default) keeps them as unstaged changes, `hard` discards \
         every uncommitted change of the index and the working tree. The window asks before a hard \
         reset; this command does not. Answers with the new HEAD.";

    fn run(&self, session: &mut Session, params: ResetParams) -> Result<HeadMoved, CommandError> {
        let revision = named(&params.revision)?;
        let repository = repository(session)?;
        if params.mode == ResetMode::Hard {
            crate::hidden_files::guard_revision(session, &repository, revision)?;
            crate::hidden_files::guard_change(session, repository.root(), &tracked_changes(&repository))?;
        }
        let recording = match params.mode {
            ResetMode::Hard => {
                history::begin_with_files(session, &repository, tracked_changes(&repository)).map(|r| r.cleaning())
            }
            ResetMode::Soft | ResetMode::Mixed => history::begin(session, &repository),
        };
        let target = repository.resolve(revision).unwrap_or_else(|_| revision.to_owned());
        let branch = repository.head().map(|head| head.label()).unwrap_or_default();
        let (revision, mode) = (revision.to_owned(), params.mode);
        background::run(
            session,
            Step::new("reset", t!("vcs-progress-reset")),
            move |repository| repository.reset(&revision, mode.into()),
            move |session, result| {
                if result.is_ok() {
                    let mode = match mode {
                        ResetMode::Soft => MoveMode::Soft,
                        ResetMode::Mixed => MoveMode::Mixed,
                        ResetMode::Hard => MoveMode::Keep,
                    };
                    let short = target.chars().take(7).collect::<String>();
                    let done = Done::new(Operation::Reset, short).target(branch).mode(mode);
                    history::finish(session, &repository, recording, done);
                }
                changed(session);
                result.map(|head| head_moved(&repository, head)).map_err(|error| command_error(&error))
            },
        )
    }
}

fn tracked_changes(repository: &zen_vcs::Repository) -> Vec<String> {
    let Ok(status) = repository.status() else { return Vec::new() };
    let mut paths = Vec::new();
    for change in status.changes {
        if change.status == zen_vcs::FileStatus::Unversioned {
            continue;
        }
        paths.extend(change.old_path);
        paths.push(change.path);
    }
    paths.sort();
    paths.dedup();
    paths
}

/// A tag to create.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TagParams {
    /// The tag's name: `v1.0`.
    pub name: String,
    /// The commit it points at: an id (or the start of it), a branch, `HEAD~2`; `HEAD` when absent.
    #[serde(default)]
    pub revision: Option<String>,
    /// A message makes an annotated tag (with its author and date); without one the tag is a
    /// lightweight one.
    #[serde(default)]
    pub message: Option<String>,
}

pub struct CreateTag;

impl Command for CreateTag {
    type Params = TagParams;
    type Output = TagInfo;
    const ID: &'static str = "vcs.tag.create";
    const TITLE: &'static str = "Create Tag";
    const DESCRIPTION: &'static str = "Creates a tag at a commit (HEAD when `revision` is absent): an annotated one when there \
         is a `message`, a lightweight one otherwise. A name that is taken or not valid is refused. \
         Answers with the tag and its commit.";

    fn run(&self, session: &mut Session, params: TagParams) -> Result<TagInfo, CommandError> {
        let name = params.name.trim();
        if name.is_empty() {
            return Err(CommandError::new(t!("vcs-error-tag-name")));
        }
        let repository = repository(session)?;
        let revision = params.revision.as_deref().map(str::trim).filter(|revision| !revision.is_empty());
        let result = repository.create_tag(name, revision, params.message.as_deref());
        changed(session);
        result.map(|commit| TagInfo { name: name.to_owned(), commit }).map_err(|error| command_error(&error))
    }
}

/// What of a commit to copy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum CopyWhat {
    /// The full id, as hex.
    #[default]
    Sha,
    /// The whole message: the subject, and the body after an empty line.
    Message,
}

/// A commit's id or message to copy.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CopyParams {
    /// The commit: its id (or the start of it), a branch, a tag, `HEAD~2`.
    pub revision: String,
    /// `sha` (the default) or `message`.
    #[serde(default)]
    pub what: CopyWhat,
    /// Put it on the clipboard (the default); `false` only answers with the text.
    #[serde(default = "yes")]
    pub clipboard: bool,
}

fn yes() -> bool {
    true
}

/// The text copied.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Copied {
    /// The commit's id or message.
    pub text: String,
    /// Whether it was put on the clipboard.
    pub clipboard: bool,
}

#[derive(Default)]
pub struct ClipboardRequest {
    text: Option<String>,
}

impl ClipboardRequest {
    pub fn take(&mut self) -> Option<String> {
        self.text.take()
    }
}

pub struct CopyCommit;

impl Command for CopyCommit {
    type Params = CopyParams;
    type Output = Copied;
    const ID: &'static str = "vcs.commit.copy";
    const TITLE: &'static str = "Copy Commit SHA or Message";
    const DESCRIPTION: &'static str = "Copies a commit's full id (`what: sha`) or its whole message (`what: message`) to the \
         clipboard, as the commit menu does, and answers with the text; with `clipboard: false` it only \
         answers.";

    fn run(&self, session: &mut Session, params: CopyParams) -> Result<Copied, CommandError> {
        let revision = named(&params.revision)?;
        let repository = repository(session)?;
        let text = match params.what {
            CopyWhat::Sha => repository.resolve(revision).map_err(|error| command_error(&error))?,
            CopyWhat::Message => {
                let (commit, _) = repository.commit_details(revision).map_err(|error| command_error(&error))?;
                if commit.body.is_empty() { commit.subject } else { format!("{}\n\n{}", commit.subject, commit.body) }
            }
        };
        if params.clipboard {
            session.state_mut::<ClipboardRequest>().text = Some(text.clone());
        }
        Ok(Copied { text, clipboard: params.clipboard })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::testing::*;
    use super::*;

    fn two_branches() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        git(&root, &["switch", "-q", "-c", "feature"]);
        std::fs::write(root.join("f.txt"), "feature\n").unwrap();
        git(&root, &["add", "f.txt"]);
        git(&root, &["commit", "-q", "-m", "Feature", "-m", "Why it is there."]);
        git(&root, &["switch", "-q", "main"]);
        std::fs::write(root.join("a.txt"), "one\ntwo\nthree\nfour\n").unwrap();
        git(&root, &["commit", "-q", "-am", "Second"]);
        (dir, root)
    }

    #[test]
    fn cherry_pick_and_revert_answer_with_the_new_head() {
        let (_dir, root) = two_branches();
        let (registry, mut session) = super::super::tests::session(&root);
        let picked = registry.dispatch(&mut session, "vcs.commit.cherryPick", json!({ "id": "feature" })).unwrap();
        assert_eq!(picked["branch"], "main");
        let head = git(&root, &["rev-parse", "HEAD"]);
        assert_eq!(picked["head"], head.trim());
        assert!(root.join("f.txt").exists());
        let again = registry.dispatch(&mut session, "vcs.commit.cherryPick", json!({ "id": "main~1" }));
        assert!(again.is_err(), "a commit the branch has is refused");
        assert_eq!(git(&root, &["rev-parse", "HEAD"]).trim(), picked["head"], "and nothing changes");
        assert!(registry.dispatch(&mut session, "vcs.commit.cherryPick", json!({ "id": " " })).is_err());

        let reverted = registry.dispatch(&mut session, "vcs.commit.revert", json!({ "id": "HEAD" })).unwrap();
        assert_ne!(reverted["head"], picked["head"]);
        assert!(!root.join("f.txt").exists());
        assert_eq!(git(&root, &["log", "-1", "--format=%s"]).trim(), "Revert \"Feature\"");
    }

    #[test]
    fn conflicts_fail_with_their_code_and_leave_the_repository_as_git_does() {
        let (_dir, root) = two_branches();
        git(&root, &["switch", "-q", "-c", "other", "main~1"]);
        std::fs::write(root.join("a.txt"), "one\ntwo\nTHREE\n").unwrap();
        git(&root, &["commit", "-q", "-am", "Other"]);
        git(&root, &["switch", "-q", "main"]);
        let (registry, mut session) = super::super::tests::session(&root);
        let failed = registry.dispatch(&mut session, "vcs.commit.cherryPick", json!({ "id": "other" })).unwrap_err();
        let zen_core::DispatchError::Failed { error, .. } = failed else { panic!("{failed:?}") };
        assert_eq!(error.code.as_deref(), Some("conflicts"));
        assert!(error.message.contains("a.txt") && error.message.contains("Resolve…"), "{}", error.message);
        assert!(root.join(".git/CHERRY_PICK_HEAD").exists(), "the cherry-pick is in progress");
        let told = session.state_mut::<super::super::workspace::GitWorkspace>().take_conflicts();
        assert_eq!(told.as_deref(), Some(error.message.as_str()), "the window tells the user too");
        registry.dispatch(&mut session, "vcs.status.refresh", json!({ "wait": true })).unwrap();
        let status = registry.dispatch(&mut session, "vcs.status.list", json!({})).unwrap();
        assert_eq!(status["changes"][0]["status"], "conflicted");
        git(&root, &["cherry-pick", "--abort"]);

        let failed = registry.dispatch(&mut session, "vcs.commit.revert", json!({ "id": "main~1" })).unwrap_err();
        let zen_core::DispatchError::Failed { error, .. } = failed else { panic!("{failed:?}") };
        assert_eq!(error.code.as_deref(), Some("conflicts"));
        assert!(error.message.contains("Resolve…"), "{}", error.message);
        assert!(root.join(".git/REVERT_HEAD").exists());
    }

    #[test]
    fn reset_moves_the_current_branch_as_its_mode_says() {
        let (_dir, root) = two_branches();
        let (registry, mut session) = super::super::tests::session(&root);
        let first = git(&root, &["rev-parse", "main~1"]).trim().to_owned();
        let second = git(&root, &["rev-parse", "main"]).trim().to_owned();
        let moved =
            registry.dispatch(&mut session, "vcs.branch.reset", json!({ "revision": first, "mode": "soft" })).unwrap();
        assert_eq!((moved["head"].as_str(), moved["branch"].as_str()), (Some(first.as_str()), Some("main")));
        assert_eq!(git(&root, &["diff", "--cached", "--name-only"]).trim(), "a.txt", "soft: staged");
        registry.dispatch(&mut session, "vcs.branch.reset", json!({ "revision": second, "mode": "hard" })).unwrap();
        registry.dispatch(&mut session, "vcs.branch.reset", json!({ "revision": "HEAD~1" })).unwrap();
        assert_eq!(git(&root, &["diff", "--name-only"]).trim(), "a.txt", "mixed by default: unstaged");
        std::fs::write(root.join("a.txt"), "scratch\n").unwrap();
        registry.dispatch(&mut session, "vcs.branch.reset", json!({ "revision": "HEAD", "mode": "hard" })).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "one\ntwo\nthree\n", "hard: discarded");
        assert!(registry.dispatch(&mut session, "vcs.branch.reset", json!({ "revision": "nope" })).is_err());
        assert!(
            registry.dispatch(&mut session, "vcs.branch.reset", json!({ "revision": "x", "mode": "keep" })).is_err()
        );
    }

    #[test]
    fn tags_are_created_and_listed() {
        let (_dir, root) = two_branches();
        let (registry, mut session) = super::super::tests::session(&root);
        let tag = registry.dispatch(&mut session, "vcs.tag.create", json!({ "name": " v1 " })).unwrap();
        let head = git(&root, &["rev-parse", "HEAD"]).trim().to_owned();
        assert_eq!(tag, json!({ "name": "v1", "commit": head }));
        let annotated = registry
            .dispatch(&mut session, "vcs.tag.create", json!({ "name": "v0", "revision": "feature", "message": "Zero" }))
            .unwrap();
        assert_eq!(annotated["commit"], git(&root, &["rev-parse", "feature"]).trim());
        assert_eq!(git(&root, &["cat-file", "-t", "v0"]).trim(), "tag", "annotated");
        assert_eq!(git(&root, &["cat-file", "-t", "v1"]).trim(), "commit", "lightweight");
        let list = registry.dispatch(&mut session, "vcs.branch.list", json!({})).unwrap();
        assert_eq!(list["tags"].as_array().unwrap().len(), 2);
        assert!(registry.dispatch(&mut session, "vcs.tag.create", json!({ "name": "v1" })).is_err(), "taken");
        assert!(registry.dispatch(&mut session, "vcs.tag.create", json!({ "name": "" })).is_err());
    }

    #[test]
    fn copy_answers_with_the_id_or_the_message_and_asks_for_the_clipboard() {
        let (_dir, root) = two_branches();
        let (registry, mut session) = super::super::tests::session(&root);
        let id = git(&root, &["rev-parse", "feature"]).trim().to_owned();
        let copied = registry.dispatch(&mut session, "vcs.commit.copy", json!({ "revision": "feature" })).unwrap();
        assert_eq!(copied, json!({ "text": id, "clipboard": true }));
        assert_eq!(session.state_mut::<ClipboardRequest>().take(), Some(id.clone()));
        assert_eq!(session.state_mut::<ClipboardRequest>().take(), None, "taken once");

        let message = registry
            .dispatch(&mut session, "vcs.commit.copy", json!({ "revision": id, "what": "message", "clipboard": false }))
            .unwrap();
        assert_eq!(message["text"], "Feature\n\nWhy it is there.");
        assert_eq!(session.state_mut::<ClipboardRequest>().take(), None, "only answered");
        assert!(registry.dispatch(&mut session, "vcs.commit.copy", json!({ "revision": "nope" })).is_err());
    }
}
