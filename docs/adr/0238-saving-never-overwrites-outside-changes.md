# ADR-0238: Saving never overwrites a change made outside the IDE

- **Status:** Proposed
- **Date:** 2026-11-05

## Context

A review of the whole project on 2026-10-31 found that only autosave looked at the disk before it
wrote. Save, Save All, Close Tab, the agents' `save: true`, Commit and the rest wrote over whatever
was on disk, even in a file already marked as a conflict. Other writers read a file, worked, and
wrote without looking again. A windowed and a headless IDE shared one Local History store without
knowing of each other, so one's prune dropped the other's versions and their ids collided. These
all lose a person's work without saying so.

## Decision

1. **One check, in `Document::save`.** Every save of an open file passes through it, so no path can
   forget it. The file on disk is compared, by content, with what the document knows: the text it
   last read or wrote (`saved`), the text in the editor, or what Keep Memory Changes accepted (kept
   as bytes, so that a file that became unreadable as text can be settled too). A deleted file is
   no conflict: saving writes it again, and autosave skips it as it did.
2. **A difference is a conflict, never a write.** The save fails, the changes stay unsaved, the
   conflict banner offers Load File from Disk and Keep Memory Changes. An unmodified document has
   nothing to keep: its save reads the file instead of refusing.
3. **Content decides; the stamp closes the window.** A stamp (modified time, size, file id) alone
   can miss a change on a coarse clock, and reading is no slower than the write that follows, so
   the comparison is always of content. The stamp is taken before that read and compared again just
   before the rename, so a change during the slow write of the temporary file is caught as well.
4. **Refused before anything runs.** A file that would be refused gets no formatting and no Actions
   on Save; they would only change a text that cannot be saved.
5. **Writes outside documents compare too.** `zen_vfs::replace_unchanged(path, before, after)` is
   for code that holds the bytes it read; `before: None` means the file must not exist. Errors say
   `is_changed_on_disk()`.
6. **`zen_workbench::disk::edit_file(session, path, |text| ...)` is the one call for an IDE action
   that rewrites a project file** (build files, `.gitignore`): an open file is edited through its
   document, as one undo step, keeping the person's unsaved text, and saved with the check; any
   other file is read, edited and written with `replace_unchanged`. The closure returns `None` to
   change nothing. Refusals are errors to show.
7. **Temporary files are the process's own:** `.<pid>-<n>.zen-save` beside the file, created with
   `create_new` and the file's mode, so two processes never share one, a private file is never
   readable by others while it is written, and the longest file name still saves.
8. **A lock around the Local History store.** Every write takes an exclusive lock on `store.lock`,
   an advisory lock the system drops if the process dies, then reads what other processes wrote;
   ids are given under it and pruning counts the versions of all of them. Without the lock (a file
   system that has none) versions are still recorded but nothing is pruned. Queries reread changed
   logs without the lock, and only a holder of the lock repairs a torn line.
9. **A folder's links are copied as links.** Following them never ended on a cycle. A link given
   as the source itself is still followed, as a copy of what it names.
10. **Rescans are not dropped.** When the system says events were lost, or the watcher reports an
    error, the window compares everything. A watcher that cannot start is tried again every 30
    seconds and reported once, not once a frame.

## Consequences

- **Behaviour people see:** ⌘S, Save All, closing a tab, quitting, Commit, Stage, Shelve and Apply
  Patch stop with an error on a file changed outside the IDE until the conflict is settled. Quitting
  stays refused until then, as it is for any file that cannot be saved. Commit and the like used to
  ignore a failed save; they now stop.
- A file changed only in its byte order mark counts as unchanged: the text is the same.
- A folder copy keeps relative links, so one pointing outside the folder may dangle in the copy.
- Build-file writers in other changes call `edit_file`, or `replace_unchanged` where there is
  no session (`jb-import`).
