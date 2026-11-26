use std::collections::BTreeSet;
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_vcs::Repository;
use zen_vcs::diff;

use super::commit::{relative, save_documents};
use super::{changed, message, repository};
use crate::Workbench;

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(Stage)?;
    registry.register(Unstage)?;
    registry.register(Discard)?;
    Ok(())
}

/// Which changes to stage, unstage or discard: whole files, every file, or some hunks and lines of
/// one file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct IndexParams {
    /// The files, absolute or relative to the repository.
    pub paths: Vec<PathBuf>,
    /// Every changed file instead of `paths`.
    pub all: bool,
    /// Only these changes of the one file in `paths`: the indices of the hunks of its diff
    /// (vcs.diff.file with `part`: `unstaged` to stage or discard, `staged` to unstage).
    pub hunks: Vec<usize>,
    /// Only the new lines with these numbers (from 0, on the diff's new side) of the one file.
    pub lines: Vec<usize>,
    /// Only the removal of the old lines with these numbers (from 0, on the diff's old side).
    pub removed_lines: Vec<usize>,
}

impl IndexParams {
    fn partial(&self) -> bool {
        !self.hunks.is_empty() || !self.lines.is_empty() || !self.removed_lines.is_empty()
    }
}

/// What was staged, unstaged or discarded.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct IndexResult {
    /// The files, relative to the repository (empty for `all`).
    pub files: Vec<String>,
    /// Whether only some lines of the file were taken.
    pub partial: bool,
    /// For an agent's discard of everything: how many untracked files (and links) it left, as an
    /// agent deletes an untracked file only by naming it (ADR-0143).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub untracked_kept: Option<usize>,
}

fn agent_targets(
    session: &Session,
    repository: &Repository,
    paths: Vec<String>,
) -> Result<Option<Vec<String>>, CommandError> {
    if !paths.is_empty() {
        crate::hidden_files::guard_change(session, repository.root(), &paths)?;
        return Ok(Some(paths));
    }
    if crate::hidden_files::hidden_changes(session, repository).is_empty() {
        return Ok(Some(paths));
    }
    let changed: Vec<String> = repository
        .status()
        .map(|status| status.changes.into_iter().map(|change| change.path).collect())
        .unwrap_or_default();
    let (kept, _) = crate::hidden_files::without_hidden(session, repository.root(), changed);
    Ok((!kept.is_empty()).then_some(kept))
}

fn targets(repository: &Repository, params: &IndexParams) -> Result<Vec<String>, CommandError> {
    let paths = if params.all {
        Vec::new()
    } else {
        params.paths.iter().map(|path| relative(repository, path)).collect::<Result<Vec<String>, CommandError>>()?
    };
    if params.partial() && paths.len() != 1 {
        return Err(CommandError::new(t!("vcs-error-one-file")));
    }
    if !params.all && paths.is_empty() {
        return Err(CommandError::new(t!("vcs-error-nothing-to-commit")));
    }
    Ok(paths)
}

fn save(session: &mut Session, repository: &Repository, paths: &[String]) -> Result<(), CommandError> {
    let paths = if paths.is_empty() {
        repository
            .status()
            .map(|status| status.changes.into_iter().map(|change| change.path).collect())
            .unwrap_or_default()
    } else {
        paths.to_vec()
    };
    save_documents(session, repository, &paths)
}

fn text(bytes: Option<Vec<u8>>, relative: &str) -> Result<String, CommandError> {
    String::from_utf8(bytes.unwrap_or_default())
        .map_err(|_| CommandError::new(t!("vcs-error-not-text", path = relative.to_owned())))
}

fn chosen(before: &str, after: &str, params: &IndexParams) -> Result<(BTreeSet<usize>, BTreeSet<usize>), CommandError> {
    let hunks = diff::hunks(before, after);
    let (mut removed, mut added) = (BTreeSet::new(), BTreeSet::new());
    for index in &params.hunks {
        let hunk = hunks.get(*index).ok_or_else(|| CommandError::new(t!("vcs-error-no-lines")))?;
        removed.extend(hunk.before.clone());
        added.extend(hunk.after.clone());
    }
    added.extend(params.lines.iter().filter(|line| hunks.iter().any(|hunk| hunk.after.contains(*line))));
    removed.extend(params.removed_lines.iter().filter(|line| hunks.iter().any(|hunk| hunk.before.contains(*line))));
    if removed.is_empty() && added.is_empty() {
        return Err(CommandError::new(t!("vcs-error-no-lines")));
    }
    Ok((removed, added))
}

fn on_disk(repository: &Repository, relative: &str) -> Option<Vec<u8>> {
    std::fs::read(super::absolute(repository.root(), relative)).ok()
}

pub(super) fn reload(session: &mut Session, repository: &Repository, paths: &[String]) {
    let workbench = session.state_mut::<Workbench>();
    for document in &mut workbench.documents {
        let listed = relative(repository, document.path()).is_ok_and(|path| paths.is_empty() || paths.contains(&path));
        if listed && document.image().is_none() {
            document.reload_from_disk();
        }
    }
}

fn failed(error: zen_vcs::VcsError) -> CommandError {
    CommandError::new(message(&error))
}

pub struct Stage;

impl Command for Stage {
    type Params = IndexParams;
    type Output = IndexResult;
    const ID: &'static str = "vcs.index.stage";
    const TITLE: &'static str = "Stage";
    const DESCRIPTION: &'static str = "Stages changes for the next commit (vcs.commit.create with `staged`): whole files (`paths`; \
         new files are added, deleted ones removed), every change (`all`), or only some `hunks`, \
         `lines` and `removedLines` of one file, numbered as its unstaged diff (vcs.diff.file with \
         `part: unstaged`) numbers them. Unsaved edits of the files are saved first.";

    fn run(&self, session: &mut Session, params: IndexParams) -> Result<IndexResult, CommandError> {
        let repository = repository(session)?;
        let Some(paths) = agent_targets(session, &repository, targets(&repository, &params)?)? else {
            return Ok(IndexResult { partial: false, files: Vec::new(), untracked_kept: None });
        };
        save(session, &repository, &paths)?;
        let result = if params.partial() {
            let file = &paths[0];
            stage_lines(&repository, file, &params)
        } else {
            repository.stage(&paths).map_err(failed)
        };
        changed(session);
        result?;
        Ok(IndexResult { partial: params.partial(), files: paths, untracked_kept: None })
    }
}

fn stage_lines(repository: &Repository, file: &str, params: &IndexParams) -> Result<(), CommandError> {
    let staged = text(repository.file_in_index(file).map_err(failed)?, file)?;
    let yours = text(on_disk(repository, file), file)?;
    let (removed, added) = chosen(&staged, &yours, params)?;
    let new = diff::combine(&staged, &yours, &|line| removed.contains(&line), &|line| added.contains(&line));
    repository.set_file_in_index(file, &new).map_err(failed)
}

pub struct Unstage;

impl Command for Unstage {
    type Params = IndexParams;
    type Output = IndexResult;
    const ID: &'static str = "vcs.index.unstage";
    const TITLE: &'static str = "Unstage";
    const DESCRIPTION: &'static str = "Takes staged changes out of the next commit, keeping them in the working tree: whole files \
         (`paths`; added files are no longer added), every staged change (`all`), or only some \
         `hunks`, `lines` and `removedLines` of one file, numbered as its staged diff (vcs.diff.file \
         with `part: staged`) numbers them.";

    fn run(&self, session: &mut Session, params: IndexParams) -> Result<IndexResult, CommandError> {
        let repository = repository(session)?;
        let Some(paths) = agent_targets(session, &repository, targets(&repository, &params)?)? else {
            return Ok(IndexResult { partial: false, files: Vec::new(), untracked_kept: None });
        };
        let result = if params.partial() {
            unstage_lines(&repository, &paths[0], &params)
        } else {
            repository.unstage(&paths).map_err(failed)
        };
        changed(session);
        result?;
        Ok(IndexResult { partial: params.partial(), files: paths, untracked_kept: None })
    }
}

fn unstage_lines(repository: &Repository, file: &str, params: &IndexParams) -> Result<(), CommandError> {
    let committed = repository.file_at_head(file).map_err(failed)?;
    let new_file = committed.is_none();
    let head = text(committed, file)?;
    let staged = text(repository.file_in_index(file).map_err(failed)?, file)?;
    let (removed, added) = chosen(&head, &staged, params)?;
    let new = diff::combine(&head, &staged, &|line| !removed.contains(&line), &|line| !added.contains(&line));
    if new_file && new.is_empty() {
        return repository.unstage(&[file.to_owned()]).map_err(failed);
    }
    repository.set_file_in_index(file, &new).map_err(failed)
}

pub struct Discard;

impl Command for Discard {
    type Params = IndexParams;
    type Output = IndexResult;
    const ID: &'static str = "vcs.changes.discard";
    const TITLE: &'static str = "Discard Changes";
    const DESCRIPTION: &'static str = "Discards changes that are not staged, at once (the Git workspace asks first through \
         vcs.workspace.discard): whole files (`paths`) get what is staged for them back, and \
         unversioned ones are deleted; `all` does so for every file; `hunks`, `lines` and \
         `removedLines` discard only those of one file, numbered as its unstaged diff (vcs.diff.file \
         with `part: unstaged`) numbers them. Staged changes stay (vcs.changes.rollback discards \
         those too). Open files show the result.";

    fn run(&self, session: &mut Session, params: IndexParams) -> Result<IndexResult, CommandError> {
        let repository = repository(session)?;
        let paths = targets(&repository, &params)?;
        let agent = zen_core::macros::by_agent(session);
        let mut untracked_kept = None;
        let (paths, files) = if paths.is_empty() && agent {
            let (tracked, untracked) = unstaged_by_kind(&repository);
            untracked_kept = Some(untracked);
            let (kept, _) = crate::hidden_files::without_hidden(session, repository.root(), tracked);
            if kept.is_empty() {
                return Ok(IndexResult { partial: false, files: Vec::new(), untracked_kept });
            }
            (kept.clone(), kept)
        } else if paths.is_empty() {
            let all = unstaged(&repository);
            (paths, all)
        } else {
            crate::hidden_files::guard_change(session, repository.root(), &paths)?;
            (paths.clone(), paths)
        };
        if params.partial() {
            save(session, &repository, &paths)?;
        }
        let recording = super::history::begin_with_files(session, &repository, files.clone());
        let result = if params.partial() {
            discard_lines(&repository, &paths[0], &params)
        } else {
            repository.discard(&paths).map_err(failed)
        };
        if result.is_ok() {
            let subject = files.first().cloned().unwrap_or_default();
            let done = super::history::Done::new(super::history::Operation::Discard, subject).count(files.len());
            super::history::finish(session, &repository, recording, done);
        }
        changed(session);
        reload(session, &repository, &paths);
        result?;
        Ok(IndexResult { partial: params.partial(), files: paths, untracked_kept })
    }
}

fn unstaged_by_kind(repository: &Repository) -> (Vec<String>, usize) {
    let Ok(status) = repository.status() else { return (Vec::new(), 0) };
    let mut tracked = Vec::new();
    let mut untracked = 0;
    for change in status.changes {
        if change.status == zen_vcs::FileStatus::Unversioned {
            untracked += 1;
        } else if change.worktree.is_some() {
            tracked.push(change.path);
        }
    }
    (tracked, untracked)
}

fn unstaged(repository: &Repository) -> Vec<String> {
    repository
        .status()
        .map(|status| status.changes.into_iter().filter(|change| change.worktree.is_some()).map(|c| c.path).collect())
        .unwrap_or_default()
}

fn discard_lines(repository: &Repository, file: &str, params: &IndexParams) -> Result<(), CommandError> {
    let staged = text(repository.file_in_index(file).map_err(failed)?, file)?;
    let yours = text(on_disk(repository, file), file)?;
    let (removed, added) = chosen(&staged, &yours, params)?;
    let new = diff::combine(&staged, &yours, &|line| !removed.contains(&line), &|line| !added.contains(&line));
    let path = super::absolute(repository.root(), file);
    std::fs::write(&path, new).map_err(|error| CommandError::new(format!("{}: {error}", path.display())))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::super::testing::*;
    use super::*;

    fn parts(registry: &Registry, session: &mut Session) -> Vec<(String, Value, Value)> {
        registry.dispatch(session, "vcs.status.refresh", json!({ "wait": true })).unwrap();
        let status = registry.dispatch(session, "vcs.status.list", json!({})).unwrap();
        status["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|change| {
                (
                    change["relative"].as_str().unwrap().to_owned(),
                    change["stagedStatus"].clone(),
                    change["unstagedStatus"].clone(),
                )
            })
            .collect()
    }

    #[test]
    fn files_are_staged_and_unstaged_whole_and_listed_in_both_parts() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        let (registry, mut session) = super::super::tests::session(&root);
        std::fs::write(root.join("a.txt"), "one\ntwo\nthree\nfour\n").unwrap();
        std::fs::write(root.join("new.txt"), "new\n").unwrap();
        assert!(registry.dispatch(&mut session, "vcs.index.stage", json!({})).is_err(), "nothing named");
        let staged = registry.dispatch(&mut session, "vcs.index.stage", json!({ "paths": ["a.txt"] })).unwrap();
        assert_eq!(staged, json!({ "files": ["a.txt"], "partial": false }));
        std::fs::write(root.join("a.txt"), "one\ntwo\nthree\nfour\nfive\n").unwrap();
        assert_eq!(
            parts(&registry, &mut session),
            [
                ("a.txt".to_owned(), json!("modified"), json!("modified")),
                ("new.txt".to_owned(), json!(null), json!("unversioned")),
            ],
            "a file changed after staging is in both lists"
        );
        registry.dispatch(&mut session, "vcs.index.stage", json!({ "all": true })).unwrap();
        assert_eq!(
            parts(&registry, &mut session),
            [("a.txt".to_owned(), json!("modified"), json!(null)), ("new.txt".to_owned(), json!("added"), json!(null)),]
        );
        registry.dispatch(&mut session, "vcs.index.unstage", json!({ "paths": [root.join("new.txt")] })).unwrap();
        assert_eq!(parts(&registry, &mut session)[1], ("new.txt".to_owned(), json!(null), json!("unversioned")));
        registry.dispatch(&mut session, "vcs.index.unstage", json!({ "all": true })).unwrap();
        assert!(parts(&registry, &mut session).iter().all(|(_, staged, _)| staged.is_null()));
    }

    #[test]
    fn hunks_and_lines_are_staged_unstaged_and_discarded_from_the_diffs() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        let (registry, mut session) = super::super::tests::session(&root);
        std::fs::write(root.join("a.txt"), "one\n2\nthree\nfour\n").unwrap();
        let unstaged =
            registry.dispatch(&mut session, "vcs.diff.file", json!({ "path": "a.txt", "part": "unstaged" })).unwrap();
        assert_eq!(unstaged["part"], "unstaged");
        assert_eq!(unstaged["hunks"].as_array().map(Vec::len), Some(2));
        let params = json!({ "paths": ["a.txt"], "hunks": [1] });
        assert_eq!(registry.dispatch(&mut session, "vcs.index.stage", params).unwrap()["partial"], true);
        assert_eq!(git(&root, &["show", ":a.txt"]), "one\ntwo\nthree\nfour\n");
        let staged = registry
            .dispatch(&mut session, "vcs.diff.file", json!({ "path": root.join("a.txt"), "part": "staged" }))
            .unwrap();
        assert_eq!(
            (staged["before"].as_str(), staged["after"].as_str()),
            (Some("one\ntwo\nthree\n"), Some("one\ntwo\nthree\nfour\n"))
        );
        let unstaged = registry
            .dispatch(&mut session, "vcs.diff.file", json!({ "path": root.join("a.txt"), "part": "unstaged" }))
            .unwrap();
        assert_eq!(unstaged["hunks"].as_array().map(Vec::len), Some(1), "what is left to stage");
        registry.dispatch(&mut session, "vcs.index.stage", json!({ "paths": ["a.txt"], "lines": [1] })).unwrap();
        assert_eq!(git(&root, &["show", ":a.txt"]), "one\ntwo\n2\nthree\nfour\n");
        registry.dispatch(&mut session, "vcs.index.unstage", json!({ "paths": ["a.txt"], "lines": [2] })).unwrap();
        assert_eq!(git(&root, &["show", ":a.txt"]), "one\ntwo\nthree\nfour\n");
        assert!(
            registry.dispatch(&mut session, "vcs.index.unstage", json!({ "paths": ["a.txt"], "lines": [0] })).is_err(),
            "no change at that line"
        );
        assert!(
            registry
                .dispatch(&mut session, "vcs.index.stage", json!({ "paths": ["a.txt", "b.txt"], "hunks": [0] }))
                .is_err(),
            "parts of one file only"
        );
        let made = registry
            .dispatch(&mut session, "vcs.commit.create", json!({ "message": "Add four", "staged": true }))
            .unwrap();
        assert!(made["message"].as_str().is_some_and(|text| text.contains("Add four")));
        assert_eq!(git(&root, &["show", "HEAD:a.txt"]), "one\ntwo\nthree\nfour\n");
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "one\n2\nthree\nfour\n");
        assert!(
            registry
                .dispatch(&mut session, "vcs.commit.create", json!({ "message": "Again", "staged": true }))
                .is_err(),
            "nothing staged"
        );
        registry.dispatch(&mut session, "vcs.index.stage", json!({ "paths": ["a.txt"] })).unwrap();
        registry
            .dispatch(
                &mut session,
                "vcs.commit.create",
                json!({ "message": "Change a", "staged": true, "amend": true }),
            )
            .unwrap();
        assert_eq!(git(&root, &["log", "--format=%s"]).lines().collect::<Vec<_>>(), ["Change a", "First"]);

        std::fs::write(root.join("a.txt"), "ONE\n2\nthree\nfour\nfive\n").unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": root.join("a.txt") })).unwrap();
        registry.dispatch(&mut session, "vcs.changes.discard", json!({ "paths": ["a.txt"], "hunks": [0] })).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "one\n2\nthree\nfour\nfive\n");
        let workbench = session.state::<Workbench>().unwrap();
        assert_eq!(workbench.active_document().unwrap().text(), "one\n2\nthree\nfour\nfive\n", "the open file follows");
        registry.dispatch(&mut session, "vcs.changes.discard", json!({ "paths": ["a.txt"] })).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "one\n2\nthree\nfour\n");
    }

    #[test]
    fn staging_lines_of_a_new_file_adds_it_with_those_lines_only() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        let (registry, mut session) = super::super::tests::session(&root);
        std::fs::write(root.join("new.txt"), "first\nsecond\n").unwrap();
        registry.dispatch(&mut session, "vcs.index.stage", json!({ "paths": ["new.txt"], "lines": [0] })).unwrap();
        assert_eq!(git(&root, &["show", ":new.txt"]), "first\n");
        registry.dispatch(&mut session, "vcs.index.unstage", json!({ "paths": ["new.txt"], "lines": [0] })).unwrap();
        assert_eq!(git(&root, &["status", "--porcelain"]).trim(), "?? new.txt", "nothing left: no longer added");
        std::fs::write(root.join("bin.dat"), [0xff, 0xfe, 0x00]).unwrap();
        assert!(
            registry.dispatch(&mut session, "vcs.index.stage", json!({ "paths": ["bin.dat"], "lines": [0] })).is_err()
        );
    }

    #[test]
    fn the_workspace_asks_before_discarding() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        let (registry, mut session) = super::super::tests::session(&root);
        std::fs::write(root.join("a.txt"), "changed\n").unwrap();
        assert!(registry.dispatch(&mut session, "vcs.workspace.discard", json!({})).is_err(), "nothing named");
        let params = json!({ "paths": [root.join("a.txt")], "hunks": [0] });
        let asked = registry.dispatch(&mut session, "vcs.workspace.discard", params).unwrap();
        assert_eq!(asked, json!({ "dialog": "discardChanges", "files": ["a.txt"] }));
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "changed\n", "nothing discarded yet");
        let workspace = session.state_mut::<super::super::workspace::GitWorkspace>();
        let pending = workspace.take_discard().unwrap();
        assert_eq!((pending.paths, pending.hunks), (vec![root.join("a.txt")], vec![0]));
        assert!(workspace.take_discard().is_none(), "taken once");
    }
}
