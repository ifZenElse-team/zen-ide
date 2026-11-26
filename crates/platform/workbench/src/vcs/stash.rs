use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;

use super::{changed, command_error, message, repository};

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListStashes)?;
    registry.register(CreateStash)?;
    registry.register(ApplyStash)?;
    registry.register(PopStash)?;
    registry.register(DropStash)?;
    registry.register(RenameStash)?;
    Ok(())
}

/// A stash, as commands report it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StashInfo {
    /// `stash@{0}` for the newest.
    pub name: String,
    /// Its place in the list, 0 for the newest.
    pub index: usize,
    /// The stash's commit, as hex: `vcs.commit.details` lists the files it holds.
    pub commit: String,
    /// The commit it was made on.
    pub base: String,
    /// Its message.
    pub message: String,
    /// When it was made, in seconds since 1970.
    pub time: i64,
    /// For an agent's new stash: how many changed files hidden from agents it left in the working
    /// tree (ADR-0143).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden_files: Option<usize>,
}

impl From<zen_vcs::Stash> for StashInfo {
    fn from(stash: zen_vcs::Stash) -> Self {
        StashInfo {
            name: format!("stash@{{{}}}", stash.index),
            index: stash.index,
            commit: stash.commit,
            base: stash.base,
            message: stash.message,
            time: stash.time,
            hidden_files: None,
        }
    }
}

/// The stashes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StashList {
    /// Newest first.
    pub stashes: Vec<StashInfo>,
}

fn list(repository: &zen_vcs::Repository) -> Result<StashList, CommandError> {
    let stashes = repository.stashes().map_err(|error| CommandError::new(message(&error)))?;
    Ok(StashList { stashes: stashes.into_iter().map(StashInfo::from).collect() })
}

pub struct ListStashes;

impl Command for ListStashes {
    type Params = NoParams;
    type Output = StashList;
    const ID: &'static str = "vcs.stash.list";
    const TITLE: &'static str = "Stashes";
    const DESCRIPTION: &'static str = "Lists the stashes, newest first: name (stash@{0}), message, when, their commit and the commit they were made on.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<StashList, CommandError> {
        let repository = repository(session)?;
        list(&repository)
    }
}

/// Which stash: by its commit when given, else by its place in the list.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StashRef {
    /// Its place in the list: 0 (the newest, `stash@{0}`) when absent.
    pub index: Option<usize>,
    /// Or its commit, as vcs.stash.list gives it (or the start of it): it names the stash even
    /// when another one is made meanwhile.
    pub commit: Option<String>,
}

pub(super) fn find_stash(repository: &zen_vcs::Repository, which: &StashRef) -> Result<StashInfo, CommandError> {
    let stashes = repository.stashes().map_err(|error| command_error(&error))?;
    if stashes.is_empty() {
        return Err(CommandError::new(t!("vcs-error-no-stashes")));
    }
    let found = match which.commit.as_deref().map(str::trim).filter(|commit| !commit.is_empty()) {
        Some(commit) => stashes.into_iter().find(|stash| stash.commit.starts_with(commit)),
        None => stashes.into_iter().nth(which.index.unwrap_or(0)),
    };
    found.map(StashInfo::from).ok_or_else(|| {
        let name = match (&which.commit, which.index) {
            (Some(commit), _) if !commit.trim().is_empty() => commit.trim().to_owned(),
            (_, index) => format!("stash@{{{}}}", index.unwrap_or(0)),
        };
        CommandError::new(t!("vcs-error-no-stash", name = name.as_str()))
    })
}

/// What to stash.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CreateParams {
    /// The stash's message; git's `WIP on <branch>: <commit> <subject>` when absent.
    pub message: Option<String>,
    /// Only the staged changes; the unstaged ones stay in the working tree.
    pub staged_only: bool,
    /// With all changes, the untracked files too (the default); `false` leaves them where they are.
    pub include_untracked: bool,
}

impl Default for CreateParams {
    fn default() -> Self {
        CreateParams { message: None, staged_only: false, include_untracked: true }
    }
}

pub struct CreateStash;

impl Command for CreateStash {
    type Params = CreateParams;
    type Output = StashInfo;
    const ID: &'static str = "vcs.stash.create";
    const TITLE: &'static str = "Stash Changes";
    const DESCRIPTION: &'static str = "Puts the changes of the working tree aside as a new stash (git stash push), under \
         `message` if given: every change, untracked files too unless `includeUntracked` is false, or \
         with `stagedOnly` only the staged changes (the others stay). The working tree is left as \
         HEAD has it. Answers with the new stash, stash@{0}; nothing to stash is refused.";

    fn run(&self, session: &mut Session, params: CreateParams) -> Result<StashInfo, CommandError> {
        let repository = repository(session)?;
        if params.staged_only {
            let status = repository.status().map_err(|error| command_error(&error))?;
            if !status.changes.iter().any(|change| change.staged) {
                return Err(CommandError::new(t!("vcs-error-nothing-staged")));
            }
        }
        let message = params.message.as_deref().map(str::trim).filter(|message| !message.is_empty());
        let hidden = crate::hidden_files::hidden_changes(session, &repository);
        let result = repository.stash_push_except(message, params.staged_only, params.include_untracked, &hidden);
        changed(session);
        let agent = crate::hidden_files::for_caller(session).is_some();
        match result {
            Ok(Some(stash)) => Ok(StashInfo { hidden_files: agent.then_some(hidden.len()), ..stash.into() }),
            Ok(None) => Err(CommandError::new(t!("vcs-error-nothing-to-stash"))),
            Err(error) => Err(command_error(&error)),
        }
    }
}

/// A stash brought back.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StashApplied {
    /// The stash, as it was before.
    pub stash: StashInfo,
    /// Whether it was dropped (a pop that applied cleanly).
    pub dropped: bool,
}

fn apply(session: &mut Session, which: &StashRef, pop: bool) -> Result<StashApplied, CommandError> {
    let repository = repository(session)?;
    let stash = find_stash(&repository, which)?;
    if let Ok(paths) = repository.stash_paths(&stash.commit) {
        crate::hidden_files::guard_change(session, repository.root(), &paths)?;
    }
    let recording = super::history::begin_stash(session, &repository, &stash.commit);
    let result = repository.stash_apply(stash.index, pop);
    if let (Err(zen_vcs::VcsError::Conflicts { .. }), Some(recording)) = (&result, recording) {
        super::history::keep_stash(session, &repository, recording, pop);
    }
    changed(session);
    match result {
        Ok(()) => Ok(StashApplied { stash, dropped: pop }),
        Err(error) => Err(super::reported(session, &error)),
    }
}

pub struct ApplyStash;

impl Command for ApplyStash {
    type Params = StashRef;
    type Output = StashApplied;
    const ID: &'static str = "vcs.stash.apply";
    const TITLE: &'static str = "Apply Stash";
    const DESCRIPTION: &'static str = "Brings a stash's changes back to the working tree and keeps the stash (git stash apply): \
         the stash at `index` (0, the newest, when absent) or the one whose commit is `commit`. \
         Changes in the way are refused; on conflicts the files are left conflicted as git leaves \
         them and the command fails with the code `conflicts` and what to do.";

    fn run(&self, session: &mut Session, params: StashRef) -> Result<StashApplied, CommandError> {
        apply(session, &params, false)
    }
}

pub struct PopStash;

impl Command for PopStash {
    type Params = StashRef;
    type Output = StashApplied;
    const ID: &'static str = "vcs.stash.pop";
    const TITLE: &'static str = "Pop Stash";
    const DESCRIPTION: &'static str = "Brings a stash's changes back to the working tree and drops the stash (git stash pop): \
         the stash at `index` (0, the newest, when absent) or the one whose commit is `commit`. On \
         conflicts git keeps the stash; the files are left conflicted and the command fails with the \
         code `conflicts` and what to do.";

    fn run(&self, session: &mut Session, params: StashRef) -> Result<StashApplied, CommandError> {
        apply(session, &params, true)
    }
}

pub struct DropStash;

impl Command for DropStash {
    type Params = StashRef;
    type Output = StashList;
    const ID: &'static str = "vcs.stash.drop";
    const TITLE: &'static str = "Drop Stash";
    const DESCRIPTION: &'static str = "Drops a stash, and its changes with it (git stash drop): the stash at `index` (0, the \
         newest, when absent) or the one whose commit is `commit`. The window asks first; this \
         command does not. Answers with the stashes left.";

    fn run(&self, session: &mut Session, params: StashRef) -> Result<StashList, CommandError> {
        let repository = repository(session)?;
        let stash = find_stash(&repository, &params)?;
        let result = repository.stash_drop(stash.index);
        changed(session);
        result.map_err(|error| command_error(&error))?;
        list(&repository)
    }
}

/// A stash's new message.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RenameParams {
    /// The stash's place in the list: 0 (the newest) when absent.
    pub index: Option<usize>,
    /// Or its commit.
    pub commit: Option<String>,
    /// The new message: the stash is then named `On <branch>: <message>`, keeping the branch it was
    /// made on.
    pub message: String,
}

pub struct RenameStash;

impl Command for RenameStash {
    type Params = RenameParams;
    type Output = StashInfo;
    const ID: &'static str = "vcs.stash.rename";
    const TITLE: &'static str = "Rename Stash";
    const DESCRIPTION: &'static str = "Gives a stash a new message (`On <branch>: <message>`, keeping the branch it was made \
         on): the stash at `index` (0, the newest, when absent) or the one whose commit is `commit`. \
         The list keeps its order and every stash its changes and time. Answers with the stash.";

    fn run(&self, session: &mut Session, params: RenameParams) -> Result<StashInfo, CommandError> {
        let text = params.message.trim();
        if text.is_empty() {
            return Err(CommandError::new(t!("vcs-error-stash-message")));
        }
        let repository = repository(session)?;
        let stash = find_stash(&repository, &StashRef { index: params.index, commit: params.commit.clone() })?;
        let result = repository.stash_rename(stash.index, text);
        changed(session);
        result.map(StashInfo::from).map_err(|error| command_error(&error))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::testing::*;

    #[test]
    fn stashes_are_created_applied_popped_renamed_and_dropped() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        let (registry, mut session) = super::super::tests::session(&root);
        let s = &mut session;
        let run =
            |s: &mut zen_core::Session, command: &str, params: serde_json::Value| registry.dispatch(s, command, params);
        assert!(run(s, "vcs.stash.create", json!({})).is_err(), "nothing to stash");
        assert!(run(s, "vcs.stash.pop", json!({})).is_err(), "no stash to pop");

        std::fs::write(root.join("a.txt"), "changed\n").unwrap();
        std::fs::write(root.join("new.txt"), "new\n").unwrap();
        let made = run(s, "vcs.stash.create", json!({ "message": "Halfway" })).unwrap();
        assert_eq!((made["name"].as_str(), made["message"].as_str()), (Some("stash@{0}"), Some("On main: Halfway")));
        assert_eq!(made["base"], git(&root, &["rev-parse", "HEAD"]).trim());
        assert!(!root.join("new.txt").exists());
        let halfway = made["commit"].as_str().unwrap().to_owned();

        assert!(run(s, "vcs.stash.create", json!({ "stagedOnly": true })).is_err(), "nothing staged");
        std::fs::write(root.join("a.txt"), "staged\n").unwrap();
        git(&root, &["add", "a.txt"]);
        std::fs::write(root.join("b.txt"), "untracked\n").unwrap();
        let staged = run(s, "vcs.stash.create", json!({ "stagedOnly": true })).unwrap();
        assert!(staged["message"].as_str().unwrap().starts_with("WIP on main"));
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "one\ntwo\nthree\n");
        assert!(root.join("b.txt").exists(), "the rest stays");
        assert!(run(s, "vcs.stash.create", json!({ "includeUntracked": false })).is_err());
        std::fs::remove_file(root.join("b.txt")).unwrap();

        let renamed = run(s, "vcs.stash.rename", json!({ "commit": &halfway[..10], "message": "Almost" })).unwrap();
        assert_eq!((renamed["index"].as_u64(), renamed["message"].as_str()), (Some(1), Some("On main: Almost")));
        assert!(run(s, "vcs.stash.rename", json!({ "index": 1, "message": " " })).is_err());
        let list = run(s, "vcs.stash.list", json!({})).unwrap();
        assert_eq!(list["stashes"][1]["commit"], halfway.as_str(), "the same stash, renamed");

        let applied = run(s, "vcs.stash.apply", json!({ "index": 0 })).unwrap();
        assert_eq!(applied["dropped"], false);
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "staged\n");
        git(&root, &["reset", "-q", "--hard"]);
        let popped = run(s, "vcs.stash.pop", json!({ "commit": halfway })).unwrap();
        assert_eq!((popped["dropped"].as_bool(), popped["stash"]["index"].as_u64()), (Some(true), Some(1)));
        assert!(root.join("new.txt").exists());
        let left = run(s, "vcs.stash.drop", json!({})).unwrap();
        assert_eq!(left["stashes"], json!([]));
        assert!(run(s, "vcs.stash.drop", json!({ "index": 4 })).is_err());
    }

    #[test]
    fn a_stash_that_conflicts_fails_with_its_code_and_is_reported() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        let (registry, mut session) = super::super::tests::session(&root);
        std::fs::write(root.join("a.txt"), "one\nmine\nthree\n").unwrap();
        registry.dispatch(&mut session, "vcs.stash.create", json!({})).unwrap();
        std::fs::write(root.join("a.txt"), "one\ntheirs\nthree\n").unwrap();
        git(&root, &["commit", "-q", "-am", "Theirs"]);
        let failed = registry.dispatch(&mut session, "vcs.stash.pop", json!({})).unwrap_err();
        let zen_core::DispatchError::Failed { error, .. } = failed else { panic!("{failed:?}") };
        assert_eq!(error.code.as_deref(), Some(super::super::CONFLICTS));
        assert!(error.message.contains("a.txt"), "{}", error.message);
        let told = session.state_mut::<super::super::workspace::GitWorkspace>().take_conflicts();
        assert_eq!(told.as_deref(), Some(error.message.as_str()), "the window tells the user too");
        let list = registry.dispatch(&mut session, "vcs.stash.list", json!({})).unwrap();
        assert_eq!(list["stashes"].as_array().unwrap().len(), 1, "kept");
    }
}
