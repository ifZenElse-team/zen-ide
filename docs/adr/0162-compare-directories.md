# ADR-0162: Compare Directories compares in the background and synchronises only inside the two folders

- **Status:** Proposed
- **Date:** 2026-10-25
- **Relates to:** [ADR-0007](0007-everything-is-a-command.md),
  [ADR-0033](0033-ai-access-to-every-api.md), [ADR-0002](0002-open-source-clean-room-pure-rust.md),
  [ADR-0108](0108-file-type-registry-and-ignored-files.md), [ADR-0143](0143-hidden-files-out-of-agents-searches-and-short-tool-schemas.md)

## Context

`file.compare.files` refused folders. Compare Directories compares two folders selected in the
Project tool window (⌘D) in a folder diff: every file only on the left, only on the right,
different or equal, toolbar toggles for each state, a path filter with `!`, `&` and `|`, an action
per row (copy to the right, copy to the left, delete) and Synchronize Selected / Synchronize All.
Folders can hold tens of thousands of files, and
synchronising copies and deletes files, so where it may write has to be settled.

## Decision

1. **Two commands** in `zen_workbench::compare_folders`: `file.compare.folders` (`left`, `right`,
   optional `states` and `filter`) answers every entry with its state and each side's kind, size
   and modification time, and the counts of the whole comparison; `file.compare.synchronize`
   (`left`, `right`, `items` of `{relative, action}`, `confirm`) copies (`toRight`, `toLeft`) or
   deletes (`delete`, from the one side that has the entry). Both take paths relative to the
   project.
2. **How files compare.** A different size is different without reading; the same size and the
   same modification time (full precision) is equal without reading; anything else is read and
   compared byte by byte. `contentsRead` says how many pairs were read. A folder on both sides is
   compared inside; a folder on one side only is one entry (copied or deleted as a whole). A
   symbolic link is an entry of its own, compared by where it points, never followed. A copy keeps
   the source's modification time, so a copied file is equal by the fast path afterwards.
3. **Ignored names** are left out of the comparison and out of copied folders through one
   function, `compare_folders::is_skipped`: `zen_vfs::HIDDEN` and `zen_vfs::is_ignored` (File Types
   → Ignored Files and Folders, ADR-0108). **Files hidden from agents** (ADR-0143) are left out of
   what an agent is told (`file.compare.folders`, the tab's state), with their number in
   `hiddenFiles`; an entry that is hidden on either side, or a folder holding a hidden file, is
   left out, and an agent's synchronisation touching one is refused (`hidden_files::guard_change`).
4. **Writes stay inside the two folders.** An entry's path must be relative, without `.` or `..`,
   and must not pass through a symbolic link or a file below the folder; a copy goes to a temporary
   file next to its target (`.<name>.zen-sync`) and is renamed over it, so a link at the target is
   replaced, never written through; deleting removes the entry itself (a link, not what it points
   at) for good. Nothing goes to the system Trash, which is outside the folders.
5. **Asking first.** `file.compare.synchronize` without `confirm: true` does nothing when an item
   would overwrite or delete, and answers those items in `needsConfirmation`. The window shows a
   confirmation dialog listing them (⏎ synchronises, ⎋ cancels); agents get a refusal naming them
   and run the action again with `confirm: true`.
6. **The tab.** Compare Directories is an editor tab beside the diff tab (`folder_diff.rs`). The
   window compares on a thread of its own by calling `compare_folders::compare`, the function the
   command runs, with a `Progress` it can cancel: commands run on the session's thread, which a
   10,000-file comparison would hold for seconds. Agents' window actions compare at once, so their
   answer has the result. Rows are drawn only where they are visible. The list is compared again
   after a synchronisation, on Refresh and whenever the tab comes back to the front (after the file
   diff it opened, or another tab). Equal files are hidden until their toggle is on. A row's default
   action copies an entry to the side that lacks it and a different file to the right; a click on
   the action cycles through what the row allows. The filter matches `*`/`?` wildcards against the
   name (the whole path when the term has a `/`) and plain terms against any part of the path,
   ignoring case.
7. **Window actions** for agents (ADR-0033): `view.folderDiff.compare`, `.refresh`, `.filter`,
   `.select`, `.setAction`, `.synchronize` and `.open`; `get_window_state` has the tab as
   `folderDiff`. ⌘D with two folders selected and `view.project.compareWith` with two folders open
   the tab; ⌘D on one folder asks for the other in a folder dialog.

## Consequences

- A file with the same size and modification time on both sides but different bytes is shown as
  equal; Refresh after touching it, or comparing by contents only, would be a later option.
- Deleting through Synchronize cannot be undone from the Trash.
- Window-only actions are not available in a headless IDE; agents there use the two commands.
