# ADR-0070: Worktrees and submodules in the Git workspace

- **Status:** Proposed
- **Date:** 2026-10-19
- **Relates to:** [ADR-0057](0057-git-workspace.md), [ADR-0047](0047-version-control-gitoxide-and-git.md),
  [ADR-0064](0064-undo-git-operations.md), [ADR-0033](0033-ai-access-to-every-api.md)

## Context

Stages 14 and 15 of the Git workspace ([feature map](../git-workspace.md)), worktrees and
submodules, ask for WORKTREES and SUBMODULES sections in the left panel: worktrees made from a
branch, opened in a project window of their own, removed (with their branch when asked), locked and unlocked, and a
WIP row per worktree with changes; submodules with whether each is at the recorded commit, updated,
initialised and opened as a project. They raise questions earlier stages did not: how to read the
state of other working trees without slowing the window, where their work in progress goes in the
graph, how a command opens another project window, and what Undo can take back.

## Decision

1. **Read and written through the git program.** `git worktree list --porcelain -z` lists the
   worktrees (lock reasons may hold new lines, hence `-z`); `git worktree add`, `remove`, `lock` and
   `unlock` change them. `git submodule status` lists the submodules with their state against the
   recorded commit, `.gitmodules` (through `git config --file`) gives their names and addresses, and
   the index the recorded commit of one that moved; `git submodule update [--init] [--recursive]`
   updates them. git keeps this bookkeeping (lock files, `modules/`, the gitlinks) and is the
   authority on it; gitoxide reads the status of each worktree, as for the project's own.
2. **Changes in other worktrees are counted in the background.** Each worktree's changed files are
   counted on a worker thread (a status of each), when the worktrees' own files change (`HEAD`,
   `index`, `locked` under `.git/worktrees/`), when this repository's status is computed again,
   when an operation changed them, and every five seconds while the Git workspace is shown (an edit
   in another worktree is noticed no other way). `vcs.worktree.list` answers with the last count at
   once and `wait` waits for the next. The repository's stamp includes the worktrees' and the
   submodules' bookkeeping, so the window's cheap poll notices a worktree or a submodule moved
   elsewhere.
3. **Another worktree's work in progress is a row of the graph**, laid out like a stash's: a side
   row right above the commit that worktree is at, in a lane of its own, joined to it (the
   project's own WIP stays the row on top). Which worktrees have changes, and where, is part of
   what the graph is read for, so the graph is laid out again when that set changes, not when only
   the counts do; the counts come from `vcs.worktree.list`.
4. **Opening a worktree or a submodule asks the window.** `vcs.worktree.open` and
   `vcs.submodule.open` (and Create Worktree's `open`) leave the folder in a session slot; the
   window takes it in its next frame and opens it as a project in a window of its own, whatever the
   "open projects in tabs" setting says, or brings it to the front when it is open. Agents then
   address that window by its folder.
5. **Undo records what moves branches.** Removing a worktree with its branch records the branch's
   deletion in ADR-0064's journal (`Delete branch 'x'`): Undo brings the branch back at its commit,
   with its upstream; the worktree's folder is not restored (its changes were said to be lost
   before). Creating a worktree is not recorded: undoing the new branch would leave a worktree on a
   branch that no longer exists, and Remove Worktree is the way back. Locking, unlocking and
   updating submodules change no branch of this repository and are not recorded.
6. **Safety.** The main worktree and the project's own cannot be removed, the main one has no lock,
   a locked worktree is refused until unlocked; one with changes is removed only with `force`,
   which the window's dialog sets after saying how many changed files are lost. A worktree's folder
   is never chosen for the user without showing it: the default, next to the repository's folder,
   is in the dialog to change.
7. **Commands for everyone** (ADR-0033): `vcs.worktree.list`, `create`, `remove`, `lock`, `open`;
   `vcs.submodule.list`, `update`, `open`; the menus `vcs.workspace.worktreeMenu`,
   `worktreeAction`, `submoduleMenu`, `submoduleAction`; the branch menu's `createWorktree`; and
   `vcs.workspace.section`, which folds and unfolds the left panel's sections (all start folded),
   so that an agent sees the panel as a user does.

## Consequences

- A repository with many worktrees of a large project costs a status of each in the background
  when something changes; the window never waits for it.
- A worktree's WIP row shows only for a commit the graph shows (a worktree detached at a commit no
  branch or tag reaches has none).
- Updating a submodule may fetch from its remote: it is an operation the user asks for, run in the
  background with its progress like Fetch (golden rule 10).

## Alternatives considered

- **Reading `.git/worktrees/*` and `.git/modules/*` ourselves.** Possible, but the formats are
  git's internals (lock files, gitdir files, the gitlinks' states); the porcelain output is git's
  stable interface for them.
- **The other worktrees' changes in the WIP panel of this one.** The graph shows them;
  committing them belongs in that worktree's own window, which Open in New Window gives.
- **Recording Create Worktree for Undo by removing the worktree.** It would delete a folder the user
  may have started working in; refused for the same reason Undo never loses work (ADR-0064).
