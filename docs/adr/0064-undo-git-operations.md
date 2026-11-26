# ADR-0064: Undo and Redo of Git operations

- **Status:** Accepted
- **Date:** 2026-10-19
- **Relates to:** [ADR-0057](0057-git-workspace.md), [ADR-0047](0047-version-control-gitoxide-and-git.md),
  [ADR-0033](0033-ai-access-to-every-api.md), [ADR-0007](0007-everything-is-a-command.md)

## Context

The Git workspace (ADR-0057) has Undo and Redo on its toolbar:
take back the last Git operation, and do it again. ADR-0057 left
the decision of what is recorded and what can be undone safely to its own ADR.

Git has no undo. What it has is the reflog: every move of `HEAD` and of a branch is written down
with the commit before and after. Undoing an operation means putting `HEAD` and the branches back
where they were, and, for operations that throw away uncommitted changes, putting those changes
back too, which the reflog cannot do. Undo must never lose work: not changes made since, not
commits made since in a terminal, not files the user edited after the operation.

Questions this decision answers: which operations can be undone; what is recorded and where; how
an operation is taken back; when Undo refuses; how the user and agents reach it.

## Decision

1. **Recorded where the operation runs.** The commands that change the repository record what they
   did in a journal (`zen_workbench::vcs::history`) right after they succeed, whoever ran them: the
   Git workspace, the Git tool window's Log, the Commit tool window, the branches popup, Find Action
   or an agent. The commands are, with the name the toolbar shows:
   - `vcs.branch.checkout`, `vcs.revision.checkout`: Checkout 'feature' (a remote branch's local
     branch made by the checkout goes away again on Undo);
   - `vcs.commit.create`: Commit 'Fix login', Amend Commit 'Fix login';
   - `vcs.branch.reset`: Reset 'main' to 1a2b3c4 (soft, mixed and hard);
   - `vcs.branch.create`, `vcs.branch.delete`: Create Branch 'x', Delete Branch 'x';
   - `vcs.branch.merge` (also into another branch), `vcs.branch.rebase` (also of another branch),
     `vcs.branch.fastForward`: Merge 'a' into 'b', Rebase 'b' onto 'a', Fast-forward 'b';
   - `vcs.commit.cherryPick`, `vcs.commit.revert`: Cherry-Pick 'subject', Revert 'subject';
   - `vcs.changes.discard` (whole files, hunks or lines, unversioned files deleted with them) and
     `vcs.changes.rollback`: Discard Changes in 'a.go', Rollback 2 Files.

   An operation that stops on conflicts is not recorded (the repository is in the middle of it; git
   has its own `--abort`), nor one that changed nothing. Operations that talk to a remote (fetch,
   pull, push, deleting a remote branch), stashes (which are their own undo), tags and renames are
   not recorded for now.
2. **What an entry keeps.** Where `HEAD` was (its branch, or a detached commit) and every local
   branch's commit before and after, the upstreams of the branches, what git wrote to `HEAD`'s
   reflog for it (`commit: Fix login`), and for discard, rollback and a hard reset the files it
   changed: the index's and the working tree's version of each, before and after. The contents of
   those files are copied into the data folder, named by their Git blob id, so that discarded
   changes and deleted unversioned files can come back even after `git gc` has pruned objects
   nothing refers to. Each repository's journal (`vcs-history/<name>-<hash>/journal.json`, its
   copies in `files/`) keeps the newest 100 operations; copies no entry needs are deleted. The
   journal outlives the window and is shared by the windows of the repository.
3. **How it is taken back.** Undo moves `HEAD` and the branches from the "after" state to the
   "before" state, Redo the other way (`zen_vcs::Repository::move_refs`): the branch `HEAD` stays on
   moves with `git reset` (`--soft` for a commit and an amend, so that its changes come back
   staged; `--soft` or `--mixed` for a reset of that mode; `--keep` otherwise,
   which brings the files along and refuses to overwrite uncommitted changes), other branches move
   with `git update-ref` given their old value, `HEAD` switches to its branch or commit with
   `git switch` (which refuses to overwrite changes too), and branches made by the operation are
   deleted last. Files kept are put back into the index and the working tree
   (`restore_files`). A hard reset is taken as two steps: its changes are thrown away, then the
   branch moves; Undo moves the branch back and then puts the changes back. The reflog shows each
   move as `zen undo commit: Fix login` or `zen redo ...`.
4. **Refused with a reason when the repository moved since.** Before changing anything, Undo
   (and Redo) compare the repository with the state the entry left it in: `HEAD` must be on the same
   branch (or commit), every branch the operation moved, made or deleted must be where it left it,
   and every file it kept must be as it left it. Branches it did not touch may have moved. Otherwise
   the command fails with the code `moved` and says what changed ("main is at 3c4d5e6 now, not at
   1a2b3c4. The repository changed since."), and nothing changes. It also refuses while a merge,
   rebase, cherry-pick or revert is in progress. Uncommitted changes in files the move would rewrite
   are protected by git itself (`--keep`, `switch`); its message is shown and the entry stays.
5. **Commands for everyone** (ADR-0033): `vcs.history.list` (what Undo and Redo would do, the
   operations done and undone, each with its title, `HEAD` before and after, the branches moved,
   the files kept and the reflog lines), `vcs.history.undo` and `vcs.history.redo` (with an optional
   `id`, refused when another operation is next). A new operation forgets what could be redone.
6. **In the Git workspace**, Undo and Redo on the toolbar are dimmed when there is nothing to take
   back or do again, and their hints name the operation and the shortcut ("Undo Commit 'Fix login'
   ⌘Z"). While the Git tab is in front and none of its text fields has the keyboard, the keymap's
   Undo and Redo (⌘Z, ⇧⌘Z, or what the user bound them to) run `vcs.history.undo` and
   `vcs.history.redo`; elsewhere they stay the editor's.

## Consequences

- Every Git operation of the IDE that moves commits around can be taken back, from any view, by
  people and agents alike, and Undo never loses work: it refuses instead, saying why.
- Discarding changes and deleting unversioned files is no longer final: their copies stay in the
  data folder until 100 newer operations push them out.
- The data folder grows with the files discarded; large discarded files take as much room as they
  had.
- An operation made in a terminal is not recorded, and an Undo after one refuses when it touched
  the same branches. Recording the reflog's own entries (operations made elsewhere) can come later.
- Each later Git feature that changes the repository (interactive rebase, the conflict editor,
  worktrees) records its operations the same way.

## Alternatives considered

- **Only the reflog, no journal.** The reflog says how `HEAD` and branches moved, but not which
  moves make up one operation (a merge into another branch is a checkout and a merge), not what was
  created or deleted, and nothing of the files thrown away. Kept as a record inside each entry.
- **Undo by stashing.** `git stash create` keeps the working tree as a dangling commit, but cannot
  be applied over other uncommitted changes without conflicts, does not keep unversioned files
  unless asked, and is pruned by `git gc`. Per-file copies are exact and are put back file by file.
- **`git reset --hard` to go back.** Simple, but it throws away whatever changed since. `--keep`
  and `switch` refuse instead.
- **Keep the journal in the repository (`.git/`).** It would travel with the repository, but it is
  the IDE's state, not the repository's; the data folder is where the IDE keeps its state.
- **Keep the journal in memory only.** Undo after reopening the project would not work, and two
  windows of a repository would not see each other's operations.
