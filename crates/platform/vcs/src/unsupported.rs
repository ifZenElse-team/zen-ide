use std::path::Path;

use crate::git::Progress;
use crate::{
    BlameLine, Branch, ChangedFile, Head, LogEntry, LogQuery, Outgoing, RefLabel, ResetMode, Status, Tag, VcsError,
};

#[derive(Debug)]
pub struct Repository {
    root: std::path::PathBuf,
}

impl Repository {
    pub fn discover(_path: &Path) -> Option<Self> {
        None
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn stamp(&self) -> u64 {
        0
    }

    pub fn refs_stamp(&self) -> u64 {
        0
    }

    pub fn resolve(&self, _revision: &str) -> Result<String, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn stashes(&self) -> Result<Vec<crate::Stash>, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn stash_push(
        &self,
        _message: Option<&str>,
        _staged_only: bool,
        _untracked: bool,
    ) -> Result<Option<crate::Stash>, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn stash_push_except(
        &self,
        _message: Option<&str>,
        _staged_only: bool,
        _untracked: bool,
        _except: &[String],
    ) -> Result<Option<crate::Stash>, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn stash_apply(&self, _index: usize, _pop: bool) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn stash_drop(&self, _index: usize) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn stash_rename(&self, _index: usize, _text: &str) -> Result<crate::Stash, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn head(&self) -> Result<Head, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn status(&self) -> Result<Status, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn branches(&self) -> Result<Vec<Branch>, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn tags(&self) -> Result<Vec<Tag>, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn recent_branches(&self, _limit: usize) -> Result<Vec<String>, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn checkout_revision(&self, _revision: &str) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn file_at_head(&self, _relative: &str) -> Result<Option<Vec<u8>>, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn commit(&self, _message: &str, _paths: &[String], _amend: bool) -> Result<String, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn commit_as(
        &self,
        _message: &str,
        _paths: &[String],
        _amend: bool,
        _author: Option<&str>,
    ) -> Result<String, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn rollback(&self, _paths: &[String], _delete_added: bool) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn write_patch(&self, _paths: &[String], _patch: &Path) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn apply_patch(&self, _patch: &Path, _include: &[String]) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn changes_patch(&self, _paths: &[String]) -> Result<Vec<u8>, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn commit_patch(&self, _revision: &str) -> Result<(crate::Commit, Vec<u8>), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn author_ident(&self) -> Option<String> {
        None
    }

    pub fn blob_named(&self, _id: &str) -> Option<Vec<u8>> {
        None
    }

    pub fn apply_git_patch(&self, _patch: &[u8]) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn leave_conflicted(
        &self,
        _relative: &str,
        _sides: [Option<&[u8]>; 3],
        _mode: &str,
        _text: &[u8],
    ) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn patch_files(&self, _patch: &Path) -> Result<Vec<String>, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn commit_staged(&self, _message: &str, _amend: bool) -> Result<String, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn commit_staged_as(&self, _message: &str, _amend: bool, _author: Option<&str>) -> Result<String, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn stage(&self, _paths: &[String]) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn unstage(&self, _paths: &[String]) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn file_in_index(&self, _relative: &str) -> Result<Option<Vec<u8>>, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn set_file_in_index(&self, _relative: &str, _content: &str) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn discard(&self, _paths: &[String]) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn last_message(&self) -> Result<Option<String>, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn log(&self, _query: &LogQuery) -> Result<Vec<LogEntry>, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn read_history(
        &self,
        _sink: &mut dyn FnMut(crate::history::CommitRecord<'_>) -> bool,
    ) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn ref_labels(&self) -> Result<Vec<(crate::history::Oid, RefLabel)>, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn commit_details(&self, _id: &str) -> Result<(crate::Commit, Vec<ChangedFile>), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn commit_files(&self, _id: &str) -> Result<Vec<String>, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn file_in_commit(&self, _id: &str, _relative: &str) -> Result<crate::Sides, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn diff_revisions(&self, _from: &str, _to: Option<&str>) -> Result<Vec<ChangedFile>, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn file_at(&self, _revision: &str, _relative: &str) -> Result<Option<Vec<u8>>, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn file_history(
        &self,
        _relative: &str,
        _revision: Option<&str>,
        _skip: usize,
        _limit: usize,
    ) -> Result<(Vec<crate::FileRevision>, bool), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn blame(&self, _relative: &str) -> Result<Vec<BlameLine>, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn count_between(&self, _from: &str, _to: &str) -> Result<usize, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn outgoing(&self) -> Result<Outgoing, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn create_branch(&self, _name: &str, _start: Option<&str>, _checkout: bool) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn checkout(&self, _name: &str) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn merge(&self, _name: &str) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn delete_branch(&self, _name: &str, _force: bool) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn fetch(&self, _progress: &mut dyn FnMut(Progress)) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn update(&self, _mode: crate::PullMode, _progress: &mut dyn FnMut(Progress)) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn push(
        &self,
        _outgoing: &Outgoing,
        _force: bool,
        _progress: &mut dyn FnMut(Progress),
    ) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn refs_containing(&self, _id: &str) -> Result<Vec<RefLabel>, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn user(&self) -> (Option<String>, Option<String>) {
        (None, None)
    }

    pub fn update_branch(&self, _name: &str, _progress: &mut dyn FnMut(Progress)) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn cherry_pick(&self, _id: &str, _mainline: Option<u32>) -> Result<String, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn revert(&self, _id: &str, _mainline: Option<u32>) -> Result<String, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn conflicted_files(&self) -> Vec<String> {
        Vec::new()
    }

    pub fn is_ancestor(&self, _ancestor: &str, _descendant: &str) -> Result<bool, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn reset(&self, _revision: &str, _mode: ResetMode) -> Result<String, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn create_tag(&self, _name: &str, _revision: Option<&str>, _message: Option<&str>) -> Result<String, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn outgoing_of(&self, _branch: Option<&str>) -> Result<Outgoing, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn rebase(&self, _onto: &str) -> Result<String, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn merge_into(&self, _name: &str, _target: &str) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn rebase_branch(&self, _branch: &str, _onto: &str) -> Result<String, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn has_tracked_changes(&self) -> Result<bool, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn fast_forward(&self, _name: &str, _onto: Option<&str>) -> Result<String, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn rename_branch(&self, _name: &str, _new_name: &str) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn set_upstream(&self, _name: &str, _upstream: Option<&str>) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn delete_tag(&self, _name: &str) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn remotes(&self) -> Result<Vec<String>, VcsError> {
        Ok(Vec::new())
    }

    pub fn remote_urls(&self) -> Result<Vec<(String, String)>, VcsError> {
        Ok(Vec::new())
    }

    pub fn default_remote(&self) -> Result<String, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn push_tag(
        &self,
        _name: &str,
        _remote: Option<&str>,
        _progress: &mut dyn FnMut(Progress),
    ) -> Result<String, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn delete_remote_branch(&self, _name: &str, _progress: &mut dyn FnMut(Progress)) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn ref_state(&self) -> Result<crate::RefState, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn in_progress(&self) -> Option<crate::Applying> {
        None
    }

    pub fn head_log_len(&self) -> u64 {
        0
    }

    pub fn head_log_since(&self, _len: u64) -> Vec<String> {
        Vec::new()
    }

    pub fn file_states(&self, _paths: &[String], _store: Option<&Path>) -> Result<Vec<crate::FileState>, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn clean_file_states(
        &self,
        _revision: Option<&str>,
        _paths: &[String],
        _store: Option<&Path>,
    ) -> Result<Vec<crate::FileState>, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn restore_files(&self, _states: &[crate::FileState], _store: &Path) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn move_refs(
        &self,
        _from: &crate::RefState,
        _to: &crate::RefState,
        _mode: crate::Move,
        _action: &str,
    ) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }
}

impl Repository {
    pub fn rebase_commits(&self, _upstream: &str, _tip: &str) -> Result<Vec<crate::Commit>, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn oldest_first(&self, _ids: &[String]) -> Result<Vec<String>, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn interactive_rebase(&self, _script: &crate::RebaseScript) -> Result<String, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn rebase_continue(&self) -> Result<String, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn rebase_abort(&self) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn rebase_progress(&self) -> Option<crate::RebaseProgress> {
        None
    }

    pub fn worktrees(&self) -> Result<Vec<crate::Worktree>, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn worktree(&self, _path: &Path) -> Result<crate::Worktree, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn add_worktree(
        &self,
        _path: &Path,
        _revision: &str,
        _new_branch: Option<&str>,
    ) -> Result<crate::Worktree, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn remove_worktree(&self, _path: &Path, _force: bool) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn lock_worktree(&self, _path: &Path, _lock: bool, _reason: Option<&str>) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn worktree_changes(_path: &Path) -> Result<usize, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn worktrees_stamp(&self) -> u64 {
        0
    }

    pub fn submodules(&self) -> Result<Vec<crate::Submodule>, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn update_submodules(
        &self,
        _paths: &[String],
        _init: bool,
        _recursive: bool,
        _progress: &mut dyn FnMut(Progress),
    ) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn submodule(&self, _path: &str) -> Result<crate::Submodule, VcsError> {
        Err(VcsError::Unsupported)
    }
}

impl Repository {
    pub fn conflicts(&self) -> Result<Vec<crate::Conflicted>, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn blob(&self, _id: &str) -> Result<Vec<u8>, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn resolve_with(&self, _relative: &str, _text: &[u8]) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn resolve_with_side(&self, _relative: &str, _ours: bool) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn resolve_deleting(&self, _relative: &str) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn in_progress_info(&self) -> Option<crate::InProgress> {
        None
    }

    pub fn continue_operation(&self) -> Result<String, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn abort_operation(&self) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn stash_paths(&self, _commit: &str) -> Result<Vec<String>, VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn drop_stash_commit(&self, _commit: &str) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }

    pub fn forget_in_index(&self, _paths: &[String]) -> Result<(), VcsError> {
        Err(VcsError::Unsupported)
    }
}

pub fn local_offset() -> i32 {
    0
}
