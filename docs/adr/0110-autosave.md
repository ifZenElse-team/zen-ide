# ADR-0110: Autosave writes modified files by itself, never over a change on disk

- **Status:** Proposed
- **Date:** 2026-10-22
- **Relates to:** [ADR-0060](0060-local-history-store.md),
  [ADR-0033](0033-ai-access-to-every-api.md), [ADR-0084](0084-headless-ide.md)

## Context

Until 0.38.32 a modified file reached the disk only through ⌘S (Save All), closing its tab or
project, exiting or restarting. The IDE has to save by itself (Settings → Appearance &
Behavior → System Settings → Files): "Save files if the IDE is idle for [15] seconds" and "Save
files when switching to a different application or a built-in terminal", both on, with the note
"Autosave cannot be disabled completely" (the UI specification).

Writing a person's files without asking needs rules: which files, when, and what must never
happen (writing over another program's change, writing a file the IDE opened read-only,
reformatting under the caret while the person types, a Local History version per keystroke).

## Decision

1. **One saving path.** Autosave is the command `file.document.autosave`; it saves through Save
   All's path (`zen_workbench::closing::save_documents`): the Actions on Save (Optimize Imports,
   Reformat Code, at most 1 s each, a failure or a timeout reported and the file saved anyway), the
   write, and one Local History version per file it saves. There is no second save
   implementation.
2. **What autosave never writes**, reported in its answer (`skipped`, with the reason) instead of
   saved:
   - `readOnly`: a document the IDE opened read-only (an encoding it does not edit), or a file
     without write permission on disk; no notification, as nothing failed;
   - `conflict`: a file whose text on disk is neither what the IDE last read or wrote, nor the
     document's text, nor what the person kept with Keep Memory Changes, or that is no longer
     text the document can read. The disk is read as the document reads it (`Document::read_disk`,
     the same reading as the file watcher's and Load File from Disk), so a document in another
     encoding is compared in its own encoding. Autosave reads the file just before writing, so a change the file watcher has not reported yet is caught too; the
     document is then marked as a conflict, as the watcher would mark it, and waits for Load File
     from Disk or Keep Memory Changes;
   - `missing`: a file that is not on disk (deleted by another program). Zen IDE has no untitled
     documents; a document without its file is the nearest case and is not written back.
   A file that cannot be written (a read-only folder) is `failed` and named in a notification;
   the other files are saved. A failure (the write, or an Action on Save that fails or times out)
   is notified once per file: later autosaves that fail the same file only list it in their
   answer, until the file saves cleanly or stops being modified; Save All and ⌘S always notify.
3. **When.** Two triggers, both on by default, kept per IDE in `settings.json` (`autosaveOnIdle`,
   `autosaveIdleSeconds` 1–3600, default 15, `autosaveOnFrameDeactivation`):
   - **Idle:** no input from the person (keys, text, clicks, pointer moves, scrolling) in any
     window **and** no change to the modified files for the idle time. Counting changes keeps an
     agent's burst of edits (over MCP, which is not input) from being saved half-way, and lets a
     headless IDE, which has no input at all, save an agent's edits after the same quiet time.
     It runs once per quiet time: a file autosave skipped or could
     not write is tried again after new input and another quiet time, or after a change, never
     every 15 seconds by itself; so a folder made writable again gets its file saved at the next
     pause. Input in a project's own window wakes the root window, which runs the idle check, after
     the idle time; between checks nothing is drawn.
   - **Leaving the window:** a window of the IDE losing the keyboard (egui's `WindowFocused(false)`,
     which winit raises on macOS when another application becomes active), or a terminal session
     getting the keyboard (Terminal tool window). Every window's modified files are saved.
     Switching between two IDE windows also counts.
4. **No formatting while typing.** The Actions on Save run on autosave as on any save
   ("Activated On: Any save"). Autosave never runs while the person types: the
   idle trigger waits for quiet, and the other trigger means the person left the editor.
5. **The text is written as it is**: the line separators and every other byte of the document's
   text, as an explicit save writes them. A file opened through a symbolic link is written through
   it (`zen_vfs::write_atomic` resolves the link); the link stays a link.

## Consequences

- An unsaved change survives switching to a browser, a terminal or a crash after the idle time.
- An agent that edits through the IDE finds its edits on disk 15 seconds after its last edit
  without calling Save All; `file.document.autosave` saves at once and says what it skipped.
- Back up files before saving, Sync external changes and Move files to the bin, on the same settings
  page, are not part of this decision.

## Merge notes

- **On Save options (ADR-0106).** That branch runs the On Save options in
  `language::format::before_save` and adds `before_save_all`, which still applies them to the
  files after a server timeout. Autosave formats through `closing::save_documents_noting`, which
  calls `before_save_noting`; when both land, `before_save_noting` keeps returning the formatter's
  failure and also runs `on_save::before_save`, and `save_documents_noting` applies the On Save
  options to the files after a timeout as `before_save_all` does, so autosave, Save All and
  closing a tab clean files the same way. The merge conflicts only in shared files (changelogs,
  `docs/`, the settings schema in `automation.rs`, `SaveAll`, and the tests next to each other in
  `format.rs`); keep both sides.
- **EditorConfig charset.** That branch decodes and encodes documents in their
  EditorConfig charset and adds its own `Document::read_disk`. Keep that version when merging: the
  autosave conflict check then compares the disk in the document's charset, and the write goes
  through `Document::save`, which encodes. Autosave never writes a document in another encoding
  as UTF-8, whatever the merge order; before that branch, such a document is opened read-only and
  autosave skips it. That branch's Save All also calls `editor_config::before_write` (the
  `end_of_line` of `.editorconfig`) for each file; move that call into `save_documents_noting`,
  after the formatting, so that autosave and closing a tab write the same line breaks.
- Both merges were tried on 2026-10-22 with `git merge` in a scratch worktree, resolved as above,
  and the `zen-workbench` tests of formatting, On Save, EditorConfig and autosave passed there,
  with an autosave test of the On Save options
  (the caret line keeps its spaces) and one of latin1 and UTF-16LE documents (written in their
  charset; a change on disk is a conflict).
