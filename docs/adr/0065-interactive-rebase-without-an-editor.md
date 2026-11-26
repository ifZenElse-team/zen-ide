# ADR-0065: Interactive rebase without an editor

- **Status:** Accepted
- **Date:** 2026-10-19
- **Relates to:** [ADR-0057](0057-git-workspace.md), [ADR-0047](0047-version-control-gitoxide-and-git.md),
  [ADR-0064](0064-undo-git-operations.md), [ADR-0033](0033-ai-access-to-every-api.md)

## Context

The Git workspace (ADR-0057) gets an interactive rebase: a branch's recent commits in a list, each
picked, reworded, squashed or dropped, reordered, then run; the same list cherry-picks several
commits. Writes go through the
user's `git` program (ADR-0047), so that hooks, signing and configuration apply.

`git rebase -i` is built for a terminal: it opens the user's editor on its todo list, again on
the message of every reworded commit and every squash, and stops on conflicts until
`git rebase --continue`. An IDE must never leave a terminal editor waiting, and agents drive the
same list through commands (ADR-0033). Questions this decision answers: how the IDE runs it, where
the list lives, how conflicts are handed over, and how Undo sees it.

## Decision

1. **The plan is a command's state, not a file.** `vcs.rebase.plan` makes the list (the commits a
   rebase would replay, as git lists them: `rev-list --reverse --no-merges --right-only
   --cherry-pick upstream...tip`; or commits to cherry-pick, ordered oldest first by ancestry) and
   keeps it in the session; `vcs.rebase.edit`, `.reset`, `.cancel` change it; nothing touches the
   repository until `vcs.rebase.start`. Cancel is therefore always "as it was".
2. **git runs non-interactively, with everything prepared.** `zen_vcs` writes the todo list (full
   ids, git's own commands `pick`, `reword`, `squash`, `fixup`, `drop`) and each reworded message
   into `<git dir>/zen-rebase/`, then runs `git rebase -i --onto <onto> <upstream> [<branch>]` with
   - `GIT_SEQUENCE_EDITOR` = `cp '<todo>'`: git's own list is replaced by ours;
   - `GIT_EDITOR` = a small `sh` script that reads the step git is at (the last line of
     `rebase-merge/done`) and, when a message was prepared for that commit, writes it over git's;
     otherwise it leaves git's message (a squash's joined messages, a picked commit's own).

   Cherry-picking several commits is the same run with `upstream` and `onto` at `HEAD`: a todo
   list may name any commits. The same script is the editor for `git rebase --continue`, so a
   reword that stopped on conflicts keeps its new message. The files go when the rebase is over or
   aborted. Paths are single-quoted with `/`, which git's shell takes on every platform.
3. **Conflicts hand over to the work in progress.** A run that stops fails with the code
   `conflicts` (as every operation does); the editor closes, the work in progress is selected and
   shows the rebase in progress (`vcs.rebase.state`: the branch, the step, the conflicted files)
   with Continue (`vcs.rebase.continue`, once nothing is conflicted) and Abort
   (`vcs.rebase.abort`). The merge conflict editor (the next stage) takes the files on later.
4. **Undo records the whole rebase once it is over.** A rebase that stops keeps a *pending* entry
   in the repository's journal (ADR-0064: `HEAD` and the branches before it, the reflog's length);
   Continue that reaches the end records it as one operation (`interactiveRebase`,
   `cherryPickMany`, or a plain `rebase` that stopped); Abort, or any new operation once nothing is
   in progress, forgets it. Undo puts the branch back with ADR-0064's checks.

## Consequences

- No editor ever opens, whatever `core.editor` or `sequence.editor` say; agents and people run
  exactly the same plan.
- The editor script needs `sh`, `tail` and `cat`, which every git installation has (Git for
  Windows ships them).
- Merge commits are left out of a plan, as `git rebase -i` leaves them out without
  `--rebase-merges`; rebasing merges stays a later option.
- A rebase stopped before the window was restarted is still continued and aborted from the
  workspace; its pending entry is in the journal file, so Undo still sees it once it is over.

## Alternatives considered

- **`exec git commit --amend -F <file>` after each reworded pick.** Simpler, but when the pick
  turns out empty and is dropped, the amend rewrites the commit before it.
- **Rewriting history ourselves with gitoxide** (cherry-picking tree by tree): a second Git
  implementation, without hooks, signing or git's merge machinery (ADR-0047 rules it out).
- **Writing the todo list after git stops on an `edit` line**: more steps, the same result.
