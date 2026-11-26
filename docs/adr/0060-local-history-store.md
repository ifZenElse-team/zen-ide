# ADR-0060: Local History: a content-addressed store in the IDE's data folder

- **Status:** Proposed
- **Date:** 2026-10-19
- **Relates to:** [ADR-0007](0007-everything-is-a-command.md), [ADR-0033](0033-ai-access-to-every-api.md),
  [ADR-0002](0002-open-source-clean-room-pure-rust.md),
  [ADR-0051](0051-jetbrains-keymaps-and-idea-project-settings.md)

## Context

The requirement for Local History: every saved version of a file is kept for a while, with a diff
and Revert, without a version control system. The versions live in the IDE's data folder, are limited by age and size, survive restarts, and a file
or a folder has Show History.

Questions this decision answers: what is recorded and when, where and in what format the versions
are kept, how the store stays small, and how recording stays off the UI thread.

## Decision

1. **The platform keeps it, for every IDE.** `zen_workbench::local_history` records and answers;
   `zen_ui` shows Show History and Put Label…. Every capability is a `localHistory.*` command
   (docs/automation.md): `versions.list`, `version.get`, `version.diff`, `version.revert`,
   `label.put`, and `changes.record` for what the file watcher reports.
2. **What is recorded.** A version of a file is recorded when:
   - the IDE saves it (Save, Save All, the saves before a commit); the text it had before is
     recorded first when the history does not hold it yet, so that the first save can be reverted;
   - the file watcher reports a change made outside the IDE (an open file's previous text first),
     or a deletion: a deleted file keeps its history;
   - an IDE action rewrites it on disk: Replace in Files, Rename, Git Rollback and Local History's
     own Revert record the content before (`before`, labelled with the action) and after (`action`).
   A Git checkout or any other program's change is recorded as an external change by the watcher.
   Reformat on save changes the text before it is saved, so its result is the saved version.
   Nothing is recorded in `.git`, in folders named `target`, `node_modules`, `dist`, `build` or
   `vendor` (`zen_vfs::EXCLUDED`), in the project's excluded folders and patterns, outside the
   project (also through a symbolic link that leads out of it, which Revert does not write through
   either), or in another IDE's project files (`.idea/`, `.run/`, `*.iml`, the write guard of
   ADR-0051, which Revert also obeys). Files over 16 MiB are not recorded. Binary files are
   recorded and listed, without a text diff.
3. **The store** is the folder `local-history` in the IDE's data folder (next to
   `recent-projects.json`; `ZEN_DATA_DIR` moves it for tests):
   - `store.json` holds `{ "format": 1 }`; a store of a newer format is read by nobody and left alone.
   - `objects/<2 hex>/<62 hex>`: every content once, named by its SHA-256. The first byte says how the
     rest is kept: `z` for zlib (deflate, `flate2` with its pure-Rust backend, already a dependency of
     the workspace), `r` for the bytes as they are, chosen when compressing does not make it smaller.
   - `projects/<16 hex of the root's SHA-256>/project.json` holds `{ "root": "<absolute path>" }`;
     `log.jsonl` next to it is an append-only log, one JSON object a line:
     `{ id, time, path?, kind, label?, hash?, size, binary }`. `id` grows within the project; `time` is
     in milliseconds since 1970; `path` is relative to the root with `/` and absent for a label of the
     whole project; `kind` is `save`, `external`, `before`, `action`, `deleted` or `label`; `label`
     is the action's id (`replaceInFiles`, `rename`, `rollback`, `revert`) or the label's text; `hash`
     is absent for deletions and labels. A line that cannot be read is skipped; a last line left
     unfinished (the process ended while appending it) is cut off when the log is read, so the next
     line starts clean. Each line is appended with one write and no `fsync`.
   Identical contents are stored once across files and projects; a save that does not change the
   file adds nothing.
4. **Limits.** Versions older than 5 days are dropped, except the last content of a file whose
   deletion is younger (so that a deleted file can still be brought back), then the oldest versions until the stored
   contents take at most 512 MiB; contents no entry refers to are deleted, and a project left
   without entries is removed. The store is pruned when the IDE starts and every hour while it runs;
   pruning rewrites a project's log atomically.
5. **Threads.** One worker thread per IDE process owns the store: the windows send it versions to
   record (with the content in hand, or a path it reads) and never wait for it; it hashes,
   compresses and writes. A saved document hands over its text before and after as rope snapshots
   (no copy on the UI thread): `Document::save` keeps them, and whoever saves (Save, Save All, the
   saves before a commit) passes them on with one call, `local_history::record_saves`. Git code
   calls Local History in one line each (saves before a commit, Rollback).
   An external change is read from disk by the worker; when a version the IDE wrote is already
   queued with that same content (the IDE saved again before the watcher's report was handled),
   the change is the IDE's own and is not recorded as external. Queries go through
   the same thread, so they see every version recorded before them, and wait for its answer
   (the commands that list, compare and revert). Contents that must be read before a change
   (Rollback, Revert) are read by the command before it changes the file.
6. **Revert** writes the version's content to the file; an open file shows it as one edit that
   Undo takes back. For a folder, every file that changed or was deleted since the version gets its
   content of then; files created since are kept and reported.

## Consequences

- The data folder grows by at most the size limit; the user can delete `local-history` at any time.
- The age limit counts calendar days, not working days.
- Versions recorded in the last moments before the process is killed may be lost. An orderly quit
  (the application dropping its window, or the restart after an update) waits up to 2 seconds for
  the worker to write what is queued.
- Two processes of the same IDE on one data folder would append to the same logs; one IDE runs one
  process, so the store takes no lock.
- The limits are not in Settings yet; they can be added without changing the format.
