# Version control (zen_workbench::vcs). English is the source language (ADR-0024).

vcs-error-no-repository = The project is not in a Git repository.
vcs-error-outside-repository = { $path } is not in the repository.
vcs-error-no-git = Cannot run Git: { $reason }. Install Git and make sure it is on the PATH.
vcs-error-unsupported = Version control is not available here.
vcs-error-detached = HEAD is detached. Check out a branch first.
vcs-error-no-remote = The repository has no remote.
vcs-error-busy = Another Git operation is running. Wait until it is finished.
vcs-error-branch-name = Enter a name for the branch.
vcs-error-revision = Enter a tag or a revision.
vcs-error-no-log-tab = There is no such Log tab.
vcs-error-tag-name = Enter a name for the tag.
vcs-error-already-on-branch = The commit is already on the current branch.
vcs-error-cherry-pick-conflicts = Cherry-pick stopped: { $count ->
    [one] 1 file has conflicts
   *[other] { $count } files have conflicts
}: { $files }. The repository is left as Git leaves it. Resolve them (Resolve… opens the conflict editor), then Continue; or Abort to put everything back.
vcs-error-revert-conflicts = Revert stopped: { $count ->
    [one] 1 file has conflicts
   *[other] { $count } files have conflicts
}: { $files }. The repository is left as Git leaves it. Resolve them (Resolve… opens the conflict editor), then Continue; or Abort to put everything back.
vcs-error-merge-conflicts = Merge stopped: { $count ->
    [one] 1 file has conflicts
   *[other] { $count } files have conflicts
}: { $files }. The repository is left as Git leaves it. Resolve them (Resolve… opens the conflict editor), then Continue; or Abort to put everything back.
vcs-error-rebase-conflicts = Rebase stopped: { $count ->
    [one] 1 file has conflicts
   *[other] { $count } files have conflicts
}: { $files }. The repository is left as Git leaves it. Resolve them (Resolve… opens the conflict editor), then Continue; or Abort to put everything back.
vcs-error-not-fast-forward = { $branch } cannot be fast-forwarded to { $target }: it has commits of its own. Merge or rebase instead.
vcs-error-no-branch = There is no local branch { $branch }.
vcs-error-no-ref = There is no branch or tag { $name }.
vcs-error-focus-name = Name the branch or the remote to hide or solo.
vcs-error-no-refs = There is no branch or tag at this commit.
vcs-error-ref-action = This action is not available for this branch or tag.
vcs-error-no-tracking = No local branch tracks { $branch }.
vcs-error-checkout-changes = Commit or stash your changes first: { $branch } has to be checked out for this, and the changes would go along.
vcs-error-drop-same = Drop the branch on another branch.
vcs-error-drop-nothing = Nothing can be done by dropping this here: merge into a local branch, or rebase a local branch.
vcs-error-drop-action = This item does not apply to these branches.
vcs-error-stash-apply-conflicts = Applying the stash stopped: { $count ->
    [one] 1 file has conflicts
   *[other] { $count } files have conflicts
}: { $files }. The stash is kept. Resolve them (Resolve… opens the conflict editor), then Continue; or Abort to put the files back.
vcs-error-stash-pop-conflicts = Popping the stash stopped: { $count ->
    [one] 1 file has conflicts
   *[other] { $count } files have conflicts
}: { $files }. Git keeps the stash when it does not apply cleanly. Resolve them (Resolve… opens the conflict editor), then Continue, which drops it; or Abort to put the files back.
vcs-error-nothing-to-stash = There are no local changes to stash.
vcs-error-nothing-staged = There are no staged changes to stash.
vcs-error-no-stash = There is no stash { $name }.
vcs-error-no-stashes = There are no stashes.
vcs-error-stash-message = Enter a message for the stash.
vcs-error-compare-two = Pick two commits with ⌘-click, or a commit and the work in progress, to compare them.
vcs-error-not-in-working-tree = { $path } is not in the working tree.
vcs-conflicts-more = { $count ->
    [one] {" "}and 1 more
   *[other] {" "}and { $count } more
}

vcs-progress-update = Updating…
vcs-progress-fetch = Fetching…
vcs-progress-push = Pushing…
vcs-progress-checkout = Checking out…
vcs-progress-merge = Merging…
vcs-progress-rebase = Rebasing…
vcs-progress-cherry-pick = Cherry-picking…
vcs-progress-revert = Reverting…
vcs-progress-reset = Resetting…
vcs-progress-commit = Committing…
vcs-progress-delete-remote = Deleting the remote branch…

vcs-update-up-to-date = All files are up-to-date
vcs-update-done = { $count ->
    [one] Project updated: 1 commit
   *[other] Project updated: { $count } commits
}
vcs-fetch-done = Fetched successfully
vcs-branch-updated = { $branch } is up to date
vcs-push-done = { $count ->
    [one] Pushed 1 commit to { $branch }
   *[other] Pushed { $count } commits to { $branch }
}
vcs-push-new-branch = Pushed { $branch } as a new branch
vcs-push-nothing = Everything is up-to-date
vcs-push-remote-moved = { $branch } has moved since this dialog opened. Look at its new commits, then push again.
vcs-push-tag-done = Pushed tag { $tag } to { $remote }
vcs-remote-branch-deleted = Deleted { $branch } on the remote

vcs-error-outside = { $path } is not in the repository.
vcs-error-empty-message = Enter a commit message.
vcs-error-sign-off-identity = Signing off needs your name and e-mail in Git (user.name and user.email).
vcs-error-nothing-to-commit = Choose the changes to commit.
vcs-error-no-file = Open a file or say which one.
vcs-error-not-open = The file is not open in the editor.
vcs-error-no-change = There is no change there.
vcs-commit-done = { $count ->
    [0] Commit amended: { $subject }
    [one] 1 file committed: { $subject }
   *[other] { $count } files committed: { $subject }
}
vcs-diff-head = HEAD
vcs-diff-yours = Your version
vcs-diff-parent = Parent of { $commit }
vcs-diff-staged = Staged version
vcs-error-nothing-staged = Stage the changes to commit first.
vcs-error-one-file = Name one file to take lines of.
vcs-error-not-text = { $path } is not a text file: stage or discard it whole.
vcs-error-no-lines = There is no change at these lines.

# Undo and Redo of Git operations (vcs.history.*, ADR-0064).
vcs-history-op-checkout = Checkout '{ $subject }'
vcs-history-op-commit = Commit '{ $subject }'
vcs-history-op-amend = Amend Commit '{ $subject }'
vcs-history-op-reset = Reset '{ $target }' to { $subject }
vcs-history-op-branch-create = Create Branch '{ $subject }'
vcs-history-op-branch-delete = Delete Branch '{ $subject }'
vcs-history-op-merge = Merge '{ $subject }' into '{ $target }'
vcs-history-op-rebase = Rebase '{ $target }' onto '{ $subject }'
vcs-history-op-cherry-pick = Cherry-Pick '{ $subject }'
vcs-history-op-revert = Revert '{ $subject }'
vcs-history-op-fast-forward = Fast-forward '{ $subject }'
vcs-history-op-interactive-rebase = Interactive Rebase '{ $target }' onto '{ $subject }'
vcs-history-op-cherry-pick-many = { $count ->
    [one] Cherry-Pick '{ $subject }'
   *[other] Cherry-Pick { $count } Commits
}
vcs-history-op-discard = { $count ->
    [one] Discard Changes in '{ $subject }'
   *[other] Discard Changes in { $count } Files
}
vcs-history-op-rollback = { $count ->
    [one] Rollback '{ $subject }'
   *[other] Rollback { $count } Files
}
vcs-history-nothing-undo = There is no Git operation to undo.
vcs-history-nothing-redo = There is no Git operation to redo.
vcs-history-not-next-undo = The operation to undo now is { $operation }, not #{ $id }.
vcs-history-not-next-redo = The operation to redo now is { $operation }, not #{ $id }.
vcs-history-refused-undo = Cannot undo { $operation }: { $reason }
vcs-history-refused-redo = Cannot redo { $operation }: { $reason }
vcs-history-reason-in-progress = a { $operation } is in progress. Finish or abort it first.
vcs-history-reason-head = HEAD is on { $now } now, not on { $expected }. The repository changed since.
vcs-history-reason-branch = { $branch } is at { $now } now, not at { $expected }. The repository changed since.
vcs-history-reason-deleted = { $branch } was deleted since.
vcs-history-reason-created = there is a branch { $branch } now.
vcs-history-reason-file = { $file } was changed since, and its changes would be lost.
vcs-history-failed-undo = Undoing { $operation } failed: { $message }
vcs-history-failed-redo = Redoing { $operation } failed: { $message }
vcs-changelist-default = Changes
vcs-error-changelist-name = A changelist needs a name.
vcs-error-changelist-exists = There is a changelist named '{ $name }' already.
vcs-error-no-changelist = There is no changelist named '{ $name }'.
vcs-error-changelist-active = '{ $name }' is the active changelist: make another one active first.
vcs-error-no-files = Name the files to move.
vcs-error-not-in-changelist = { $path } is not a changed file in a changelist.
vcs-error-nothing-to-shelve = There are no changes to shelve.
vcs-error-shelve-rollback = Part of the changes stayed in the working tree: { $reason }. Everything is kept in the shelved changes "{ $name }", and Undo restores the files.
vcs-error-not-changed = { $path } has no changes.
vcs-error-shelve-conflicted = { $path } has conflicts: resolve them before shelving it.
vcs-error-no-shelf = There are no shelved changes with the id { $id }.
vcs-error-not-shelved = { $path } is not in these shelved changes.
vcs-error-shelf-name = Shelved changes need a name.
vcs-error-invalid-branch = "{ $name }" is not a valid branch name.
vcs-error-invalid-tag = "{ $name }" is not a valid tag name.
vcs-error-invalid-remote = "{ $name }" is not a valid remote name.
vcs-error-invalid-revision = "{ $name }" is not a valid revision.
vcs-error-unshelve-conflicts = Unshelving stopped: { $count ->
    [one] 1 file has conflicts
   *[other] { $count } files have conflicts
}: { $files }. The shelved changes are kept. Resolve the conflicts in the files.
vcs-shelf-diff-base = Base version
vcs-shelf-diff-shelved = Shelved version
vcs-history-op-shelve = Shelve '{ $subject }'
vcs-history-op-unshelve = Unshelve '{ $subject }'
vcs-history-op-apply-patch = Apply Patch '{ $subject }'

# Create Patch and Apply Patch (vcs.patch.*, ADR-0117).
vcs-patch-name = Patch
vcs-patch-conflict-current = Current
vcs-patch-conflict-patch = Patch
vcs-patch-diff-current = Current version
vcs-patch-diff-patched = After the patch
vcs-patch-error-revision = Name the commit to make a patch of.
vcs-patch-error-shelf = Name the shelved changes to make a patch of.
vcs-patch-error-no-changes = There are no changes to put in a patch.
vcs-patch-error-nothing = None of the files is in the patch.
vcs-patch-error-encoding = { $name } is not an encoding.
vcs-patch-error-base = The base path { $path } is neither inside the repository nor above it.
vcs-patch-error-no-input = Give a patch file, the patch's text or the clipboard.
vcs-patch-error-empty = There are no changes in the patch.
vcs-patch-error-hunk = Line { $line } of the patch is not a hunk header.
vcs-patch-error-truncated = The hunk at line { $line } of the patch is cut short.
vcs-patch-error-unsafe = The patch names '{ $path }', which is outside the project or in .git; it is not applied.
vcs-patch-error-strip = '{ $path }' has fewer folders than those to strip.
vcs-patch-error-outside = { $path } is outside the base path.
vcs-patch-error-large = The patch is too large.
vcs-patch-error-not-in-patch = { $path } is not in the patch.
vcs-patch-error-not-applied = The patch could not be applied: { $files }.
vcs-patch-reason-symlink = The path goes through a symbolic link; a patch never writes there.
vcs-patch-reason-special = The patch makes or changes a symbolic link or a submodule, which the IDE does not apply.
vcs-patch-reason-no-data = The patch says the binary file changed but has no data for it.
vcs-patch-reason-not-text = The file is not UTF-8 text, so what does not apply cannot be merged.
vcs-patch-reason-delete-differs = The file to delete has other content than the patch removes.
vcs-patch-reason-target-exists = A file is already where the patch moves or copies it.
vcs-error-patch-conflicts = Some hunks did not apply: { $count ->
    [one] 1 file has conflicts
   *[other] { $count } files have conflicts
}: { $files }. Merge them in the conflict editor.

# The interactive rebase editor (vcs.rebase.*, ADR-0065).
vcs-rebase-problem-meld = { $commit } cannot be squashed or fixed up: there is no commit kept before it to go into.
vcs-rebase-problem-message = Enter a message for { $commit }.
vcs-rebase-problem-nothing = Every commit is dropped: there is nothing to cherry-pick.
vcs-rebase-error-no-step = There is no such commit in the interactive rebase.
vcs-rebase-error-closed = The interactive rebase editor is not open.
vcs-rebase-error-in-progress = A { $operation } is in progress. Finish or abort it first.
vcs-rebase-error-nothing = The repository has no commits yet.
vcs-rebase-error-on-branch = { $commit } is already on the current branch.
vcs-rebase-error-merge = { $commit } is a merge commit: merges are not cherry-picked here.
vcs-rebase-error-from = { $commit } is not an earlier commit of the current branch.
vcs-rebase-error-what = Say what to rebase: onto a branch, from a commit, or commits to cherry-pick.
vcs-rebase-error-no-commits = { $branch } has no commits to rebase onto { $onto }.
vcs-rebase-error-too-many = { $count } commits are too many for one interactive rebase (at most { $limit }): rebase from a later commit.
vcs-rebase-error-twice = { $commit } is in the list twice.
vcs-rebase-error-changes = Commit or stash your changes first: a rebase rewrites the files.

## Worktrees and submodules (vcs.worktree.*, vcs.submodule.*).
vcs-error-worktree-branch-exists = There is a local branch { $branch } already: name the new branch of the worktree.
vcs-error-worktree-folder = { $folder } is not empty: choose another folder for the worktree.
vcs-error-no-worktree = { $folder } is not a worktree of this repository.
vcs-error-worktree-remove-own = The main worktree and this project's own cannot be removed.
vcs-error-worktree-locked = The worktree { $folder } is locked: unlock it first.
vcs-error-worktree-lock-main = The main worktree cannot be locked.
vcs-error-worktree-gone = The folder of the worktree { $folder } is gone.
vcs-error-no-submodule = { $path } is not a submodule of this repository.
vcs-error-no-submodules = The repository has no submodules.
vcs-error-submodule-uninitialized = The submodule { $path } is not initialised: initialise it first.
vcs-progress-submodule-update = Updating submodules…
vcs-progress-submodule-init = Initialising submodules…
vcs-submodule-updated = { $count ->
    [one] Updated { $count } submodule
   *[other] Updated { $count } submodules
}
# The merge conflict editor and the operation in progress (vcs.conflict.*, vcs.operation.*, ADR-0072).
vcs-operation-none = No merge, rebase, cherry-pick, revert or stash is in progress.
vcs-operation-merge = Merging '{ $theirs }' into '{ $ours }'
vcs-operation-rebase = Rebasing '{ $branch }' onto { $onto }: commit { $step } of { $total }
vcs-operation-cherry-pick = Cherry-picking { $commit } onto '{ $ours }'
vcs-operation-revert = Reverting { $commit } on '{ $ours }'
vcs-operation-stash-apply = Applying { $stash } on '{ $ours }'
vcs-operation-stash-pop = Popping { $stash } on '{ $ours }'
vcs-conflict-ours-rebase = onto { $onto }
vcs-conflict-error-none = There are no conflicted files.
vcs-conflict-error-not-conflicted = { $file } has no conflicts.
vcs-conflict-error-closed = The conflict editor is not open: name the file.
vcs-conflict-error-no-change = There is no change { $change } in this file.
vcs-conflict-error-not-mergeable = This file cannot be merged line by line (it is binary, or a side deleted it): take ours or theirs as a whole.
vcs-conflict-error-both-file = Both sides at once can be taken for changes only, not for the whole file.
vcs-conflict-error-unresolved = { $count ->
    [one] 1 conflict is not resolved yet
   *[other] { $count } conflicts are not resolved yet
}: take a side for it or edit the result, or mark the file resolved anyway.

git-workspace-title = Git
git-workspace-graph-tab = Graph
vcs-error-safe-mode = Git is off: the project is opened in safe mode, where the IDE makes no Git call. Trust the project (Trust Project) and Git starts without reopening it.
