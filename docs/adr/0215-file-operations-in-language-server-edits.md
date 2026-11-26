# ADR-0215: File operations in language server edits

- **Status:** Proposed
- **Date:** 2026-11-04
- **Supersedes:** point 1 of [ADR-0044](0044-how-a-refactoring-changes-files.md) (resource operations refused)

## Context

Since ADR-0044 the client declared `resourceOperations: []` and refused any `WorkspaceEdit` that
creates, renames or deletes a file. That keeps a Java class rename from renaming its `.java` file,
a Rust module rename from moving its file, and "create class from usage" quick fixes from working.
In the other direction, a file renamed, moved or deleted in the Project tool window left imports,
`mod` declarations, package and class names behind, because the servers were never asked. The
IDE must do both: a class rename renames its file, and renaming or moving a file updates
what refers to it. The Language Server Protocol has the pieces: `CreateFile`, `RenameFile` and
`DeleteFile` in `documentChanges` (with `overwrite`, `ignoreIfExists`, `recursive`,
`ignoreIfNotExists`), and `workspace/will…Files` requests and `did…Files` notifications, each
registered by the server with glob filters.

## Decision

1. **The client declares** `resourceOperations: ["create", "rename", "delete"]`,
   `failureHandling: "undo"` and `workspace.fileOperations` with every `will` and `did` operation,
   statically (`dynamicRegistration: false`). A server's own filters (`scheme`, `glob`, `matches`
   file or folder, `ignoreCase`) decide which files it hears of; the glob is matched against the
   whole path with `*`, `**`, `?`, `{a,b}` and `[...]` (`zen_lsp::file_operations`).
2. **An edit with file operations keeps its order.** `edits::parse` follows the operations in a
   plan of the file system: a text edit is resolved against the file as the earlier operations
   left it (a created file is empty, a renamed one keeps its text), the options are checked
   against that plan (`ignoreIfExists` drops a create or rename, `overwrite` wins over it,
   `ignoreIfNotExists` drops a delete, a missing source or a taken name refuses the whole edit), and
   the steps are applied one after another. Without file operations the edit is read as before:
   one change per file, in path order.
3. **Every path a file operation touches must be inside the project** (also a file not yet
   created, through its nearest existing folder), not hidden from the caller nor a folder holding
   a file hidden from it, not another IDE's
   settings and not locked. There is no confirmation to change files outside: such an edit is
   refused as a whole.
4. **All or nothing, one Undo.** If a step fails, the steps already made are reverted in reverse
   order. The journal entry of ADR-0044 holds the file operations too: a created file (and the
   folders made for it), a renamed one (with a file it replaced) and a deleted one, whose content
   is kept in memory (up to 64 MiB, else the edit is refused), so that Undo brings it back; deletes
   do not go through the Trash. Undo reverts the entry in reverse order, moving renamed files and
   their tabs back; an open document edited several times in one entry is one undo step. Undo is
   reached from a document the entry edited or renamed, by its path after the entry's renames.
   Servers hear of the files created, renamed and deleted through `didChangeWatchedFiles`.
5. **The Project tool window asks the servers first.** `file.fs.rename`, `project.file.paste`
   after Cut (a move), `file.fs.delete`, `file.fs.newFile` and `file.fs.newDirectory` send the
   `will…Files` request to every running server whose filters match, wait for the answers for at
   most 3 seconds in all (a server that does not answer is cancelled and named in a message), apply
   the edits they return as above, make the change, then send `did…Files`. A copy sends
   `didCreateFiles`. The server's edits and the rename or move are one journal entry, so one Undo
   in a changed file reverts both; if the change itself fails, the edits are reverted.
   `file.fs.rename` takes `updateReferences: false` to skip the servers.
6. **The rename preview lists the file operations** (`refactor.rename.preview` → `operations`,
   and a "Files to create, rename or delete" group in the Refactoring Preview tab).

## Consequences

- JDT LS renames a class's file with the class and a class with its file; the TypeScript server
  updates imports when a module is renamed; rust-analyzer moves a module's file and renames `mod`
  declarations when the file is renamed; quick fixes that create files work in every language.
- The `will…Files` wait blocks the command, as `refactor.rename.apply` waits for its server; a
  slow server costs at most three seconds and the change goes on without its edits.
- A deleted folder's content is held in the journal until it falls out of its 20 entries.
- Bookmarks, file type overrides and navigation history follow a rename made in the Project tool
  window, not one made or undone by a server's edit.

## Alternatives considered

- **Move deleted files to the Trash:** they could not be restored by Undo without the platform's
  Trash API, and a refactoring's deletes would fill the Trash.
- **Send `willRenameFiles` and apply its answer after the rename, without waiting:** the edits name
  the old paths and versions, so they would have to be mapped to the new ones, and the change would
  show up a moment after the rename; the protocol means the edits to be made before.
- **Ask before touching files outside the project:** a dialog for an agent's command has no one to
  answer it; refusing is safe, and a server has no reason to move files out of the project.
