# ADR-0072: The merge conflict editor

- **Status:** Accepted
- **Date:** 2026-10-19
- **Relates to:** [ADR-0057](0057-git-workspace.md), [ADR-0047](0047-version-control-gitoxide-and-git.md),
  [ADR-0064](0064-undo-git-operations.md), [ADR-0065](0065-interactive-rebase-without-an-editor.md),
  [ADR-0033](0033-ai-access-to-every-api.md)

## Context

Two requirements ask for one feature: a merge conflict editor in the Git workspace (its stage 10)
and resolving merge conflicts in a three-way Merge Revisions window, opened from the Commit tool
window and after Update or Merge. Until now a merge, a cherry-pick, a revert, a rebase or a stash that stopped
on conflicts left the files with git's markers; only a rebase had Continue and Abort, in the work in
progress panel (ADR-0065).

Questions this decision answers: where the three versions and the result come from; how the panes
line up and how the result is edited; what Continue and Abort do for each operation, including a
stash, which git does not keep in progress; and how Undo sees an operation finished after
conflicts.

## Decision

1. **The versions come from git's index.** A conflicted file has stages: 1 the common ancestor, 2
   ours, 3 theirs (`git ls-files -u`, read by `zen_vcs` `conflicts` and `blob`). Ours is always the
   branch the changes go onto (during a rebase, the commits already replayed onto `onto`), theirs
   what is applied (the branch merged in, the commit picked, reverted or replayed, the stash). The
   working tree's file with its markers is not read: whatever git or the user wrote there, the
   editor starts from the three versions.
2. **The panes are lined up by our own three-way merge** (`zen_vcs::merge`, pure Rust): each
   side's changes against the ancestor with the diff viewer's histogram diff; changes of the two
   sides that overlap or touch are one conflict, as in git's merge, and lines both sides share at a
   conflict's start and end are taken out of it. Changes of one side (or the same change of both)
   start taken; a conflict starts with the ancestor's lines and unresolved. The
   result is kept chunk by chunk, so that the three panes stay lined up whatever is taken.
3. **The result is the session's, and edits keep the chunks.** The open file and its result live in
   the session (`ConflictEditor`), so that the tab and agents see the same thing; nothing is written
   until the file is marked resolved, and closing the tab writes nothing. Typing replaces the
   result's text: the lines an edit touches (and the chunks they are in) become one edited chunk,
   which counts as resolved; everything else keeps its state. In the tab, a click on the result's
   lines edits that chunk in place.
4. **Writing goes through git** (ADR-0047). Mark Resolved writes the result and `git add`s it; a
   whole side is taken with `git checkout --ours`/`--theirs` (which keeps the mode) or `git rm` when
   that side deleted the file. A file that is not text, a link, or one a side deleted can only be
   resolved with a whole side.
5. **Continue and Abort for every operation**, one pair of commands (`vcs.operation.continue`,
   `vcs.operation.abort`) behind the banner and the work in progress panel, the interactive
   rebase's box generalised: `git merge --continue`, `cherry-pick --continue`, `revert --continue`
   (with an editor that keeps git's message; a cherry-pick or revert resolved to no change is
   skipped, as git suggests), `rebase --continue` (ADR-0065's), and their `--abort`. A stash leaves
   nothing in progress in git, so the IDE keeps it: before applying a stash, the files it touches
   are kept as ADR-0064 keeps files (their index and working tree versions, contents in the data
   folder); when it stops on conflicts that is the operation in progress. Continue then drops a
   popped stash (git kept it); Abort puts those files back and keeps the stash. Any later recorded
   operation forgets it.
6. **Undo records the whole operation once it is over.** A merge, a cherry-pick or a revert that
   stops on conflicts keeps a pending entry as a rebase already did (ADR-0065); Continue, or a
   commit that concludes it, records it as one operation; Abort forgets it. Stashes stay outside
   Undo (ADR-0064).
7. **Every action is a command** (ADR-0033): `vcs.conflict.list`, `.open`, `.get`, `.resolve`
   (`take` for a change, several, every conflict or the whole file; `text`; `markResolved`, refused
   with the code `unresolved` while conflicts are left unless `force`), `.close`, and
   `vcs.operation.continue` and `.abort`.
8. **The look.** The editor is a tab next to the Git tab, in the workspace's look (ADR-0057), with
   this layout of Merge Revisions: ours left, the result in the middle, theirs right, » and «
   between them, previous and next conflict on F7 and ⇧F7, Accept Ours, Accept Theirs and Mark
   Resolved. The panes are drawn as the diff tab draws its sides (the same highlighting, change
   marks and one sideways scroll for all panes), in the theme's colours only.

## Consequences

- Conflicts of every operation are resolved in one place, by people and agents alike; the Commit
  tool window, the conflicts dialog (also after Update Project) and the workspace lead there.
- Our merge may cut a conflict differently from git's markers (git's own refinements, its rename
  detection): the editor shows our chunks, which never lose a line of either side; the file git
  wrote is replaced only by Mark Resolved.
- A stash's Abort is exact for the files it touched, but it puts them back as they were before the
  stash was applied: edits made to those files since are lost, as with git's own `--abort`.
- `vcs.rebase.continue` and `vcs.rebase.abort` stay for agents that use them; they do what the
  general commands do for a rebase.

## Alternatives considered

- **Parse the markers in the working tree.** It depends on `merge.conflictStyle` (merge, diff3,
  zdiff3), breaks when the user edited the file, and gives nothing for files git could not mark
  (deleted, binary).
- **Use git's `merge-file`** for the chunks. It writes markers, not chunks, so we would parse them
  again, and it is one more process per keystroke of an edit.
- **A free text editor for the result.** The panes would no longer line up without
  tracking every edit against every pane; editing a chunk in place keeps them lined up and still
  lets any line be changed.
- **`git reset --merge` to abort a stash.** It also throws away changes that were staged before the
  stash was applied.
