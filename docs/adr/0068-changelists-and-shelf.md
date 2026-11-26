# ADR-0068: Changelists and the shelf

- **Status:** Proposed
- **Date:** 2026-10-19
- **Relates to:** [ADR-0047](0047-version-control-gitoxide-and-git.md), [ADR-0064](0064-undo-git-operations.md),
  [ADR-0033](0033-ai-access-to-every-api.md)

## Context

The requirement: changelists in the Commit tool
window (changes grouped in named lists, one of them active; create, rename and move files between
them; commit one list) and a shelf (changes put aside without a commit, shown on a Shelf tab,
brought back with Unshelve, with a diff preview). Git has neither: a changelist is the IDE's
grouping of uncommitted changes, and a shelf is the IDE's own store of patches, unlike a stash,
which lives in the repository's refs. Both need a place to be kept and a file format, and the
shelf needs a way to take changes out of the working tree and put them back when the files have
changed since.

Questions this decision answers: where changelists and shelves are kept and in what format; how a
change gets its list; how shelving and unshelving work with git; how conflicts are handled; what
Undo (ADR-0064) records; whether the per-user files of an `.idea` folder (`.idea/workspace.xml`,
`.idea/shelf/`) are read.

## Decision

1. **Kept in the data folder, per repository.** The changelists of a repository are
   `vcs-changelists/<name>-<hash>.json` (the same key as ADR-0064's journal): the lists in order
   (name, comment), the active one, and which list each changed file is in. The shelf is the folder
   `vcs-shelf/<name>-<hash>/`: `shelf.json` (each shelf's id, name, time, files with their status
   when shelved, and whether it was unshelved), one `<id>.patch` per shelf, and `<id>/<n>.base` and
   `<id>/<n>.shelved` (each file's version at HEAD and as shelved) for the diff preview. Files are
   written whole and renamed into place. Without a data folder (tests, WebAssembly) the lists stay
   in memory and shelves go to the temporary folder.
2. **A change gets its list once.** There is always a list ("Changes" at first). A tracked change
   that appears goes into the active list and stays in its list until it is no longer changed
   (committed, rolled back, shelved) or is moved; it then goes into the active list when it changes
   again. Unversioned files are in no list. The lists follow the status when
   they are read, waiting for a status being computed so that a file is not dropped for a moment.
   Deleting a list moves its files to the active one; the active list cannot be deleted.
3. **A shelf is a binary git patch.** Shelving runs `git diff --cached --binary --full-index -M`
   against a temporary index (`GIT_INDEX_FILE`, made from `HEAD` with the files added), so staged
   and unstaged changes and unversioned files are in it and the repository's index is untouched;
   then the files are rolled back (`git restore`, unversioned files deleted). Unsaved edits of the
   files are saved first. Unshelving runs `git apply --binary`, and when the patch no longer applies
   as it is, `git apply --3way` from the blob ids the patch names: what merges cleanly is kept and
   unstaged again; what conflicts is left with conflict markers and unmerged in the index, as a
   merge leaves it, and the command fails with the code `conflicts` (the window shows its conflicts
   dialog). A shelf is never deleted by unshelving: once applied whole it is marked "already
   unshelved" and can be unshelved again or deleted.
4. **Undo records them.** Shelving and unshelving are recorded in ADR-0064's journal as operations
   on files (`Shelve 'name'`, `Unshelve 'name'`), the files kept before and after, like a rollback:
   Undo of a shelve puts the changes back into the files (the shelf keeps them too, so they then
   exist twice); Undo of an unshelve takes them out again (the shelf still has them, marked
   unshelved). An unshelve that stops on conflicts is not recorded.
5. **The per-user files of an `.idea` folder are not read.** `.idea/workspace.xml` (its
   changelists) and `.idea/shelf/` are per-user files, which the `.idea` import ([ADR-0051](0051-jetbrains-keymaps-and-idea-project-settings.md)) leaves
   alone on purpose; reading them would make the IDE depend on another IDE's private state.
6. **Commands for everyone** (ADR-0033): `vcs.changelist.list`, `create`, `edit`, `delete`,
   `setActive`, `move`; `vcs.commit.create` with `changelist`; `vcs.shelf.list`, `create`, `diff`,
   `unshelve`, `rename`, `delete`; the dialogs through the `view.vcs.newChangelist`,
   `view.vcs.moveToChangelist` (⌥⇧M), `view.vcs.shelve`, `view.vcs.unshelve` and
   `view.vcs.showShelf` actions.

## Consequences

- Changelists and shelves survive restarts and are shared by the windows of a repository; they do
  not travel with the repository: they are per-user files.
- A shelf stays usable when the branch moves on: its patch names the blobs it was made from, which
  stay in the repository's history; the base and shelved copies keep the preview exact even if git
  prunes objects.
- Unshelving onto files with uncommitted changes of their own is refused by git when the patch does
  not apply as it is (the three-way merge needs the index to match the files); its message is shown.
- Shelves of large binary files take as much room in the data folder as the files.

## Alternatives considered

- **Shelves as stashes.** `git stash` keeps changes in the repository, but they are shared with the
  Git workspace's stash list, cannot be named per file, and `stash apply` refuses more often.
- **Changelists from the index (staged versus not).** Simple, but only two lists, and staging has a
  meaning of its own in the Git workspace.
- **Reading `.idea/workspace.xml` and `.idea/shelf/`.** Rejected for the reasons in point 5.
