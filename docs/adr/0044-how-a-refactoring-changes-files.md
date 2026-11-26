# ADR-0044: How a refactoring changes files

- **Status:** Proposed
- **Date:** 2026-10-18

## Context

Rename (⇧F6) is the first change that the language server computes for many files at once: gopls
answers `textDocument/rename` with a `WorkspaceEdit` that edits the file at the caret, other open
files and files that are not open at all. Formatting, organise imports and quick fixes will send
the same kind of edit. The IDE has to preview such a change, apply it to every file or to none,
leave the open files unsaved, and undo it everywhere with one Undo. Questions on the table:
which forms of `WorkspaceEdit` the client accepts, what happens when a file changed after the
server computed the edit, how files that are not open are changed, and how undo crosses files.

## Decision

1. **The client declares** `workspace.workspaceEdit` with `documentChanges: true`, no resource
   operations (`resourceOperations: []`) and `failureHandling: "abort"`, and
   `workspace.didChangeWatchedFiles`. `workspace.applyEdit` stays off: the IDE asks for edits, the
   server does not push them. An edit with a create, rename or delete operation (a package rename
   that moves files) is refused as a whole with a message; nothing is applied.
2. **Reading an edit** (`zen_workbench::language::edits::parse`) accepts `documentChanges` of text
   document edits and the older `changes` map. Positions (UTF-16) are resolved to character ranges
   against the text the server had: its copy of an open file, else the file on disk, read once.
   Each file's edits are sorted; edits that overlap make the whole edit refused. A versioned edit of
   a file open in the server must be at the server's current version.
3. **Applying is all or nothing** (`edits::apply`). First every file is checked: an open document
   must still have exactly the text the edit was computed against (and the server the same
   version), a file that is not open must still have that text on disk. Then files that are not
   open are written atomically (`zen_vfs::write_atomic`), keeping their old text; if one write
   fails, the files written before it are restored and nothing else changes. Last, each open
   document gets its changes as one undo step and stays unsaved. The servers are told about the
   written files with `workspace/didChangeWatchedFiles`, and the workbench revision is raised.
4. **Undo goes through a refactoring journal** (`refactor::Refactoring`, the last 20). When the
   active document's latest change belongs to a refactoring, `editor.edit.undo` undoes that
   refactoring: one undo step in every open document still at the version the refactoring left,
   and the old text written back to every file on disk that still has the refactoring's text.
   Files changed since are left alone and named in a message.
5. **Rename is four commands** (ADR-0007, ADR-0033): `refactor.rename.prepare` (the range and the
   current name, from `textDocument/prepareRename`, or the identifier at the caret when the server
   has no prepare step or asks for its default behaviour), `refactor.rename.preview` (every change
   by file, stored as the plan), `refactor.rename.apply` (the stored plan, or a new rename applied
   in one go) and `refactor.rename.cancel`. The state lives in its own session slot, not among the
   navigation lookups. The new name must be an identifier of the file's language (for Go: a letter
   or `_`, then letters, digits and `_`, not a keyword) before anything is sent.

## Consequences

- The dialog and preview of ⇧F6 are UI over these commands; agents rename with the same commands.
- Formatting and code actions can reuse `edits::text_edits` and `edits::apply_text_edits` for edits
  of one file, and `edits::parse` and `edits::apply` for workspace edits.
- A rename that moves files is not possible until resource operations are supported; that needs
  file creation, moves and deletion in the same all-or-nothing scheme, and in the journal.
- Undo of a refactoring is reached from an open document it changed; redo of a refactoring redoes
  only the active document for now.
- The identifier rule is the platform's, with keywords per language id; a language with other rules
  will need it to come from its language module.

## Alternatives considered

- **Let the server apply the edit through `workspace/applyEdit`:** the IDE would not see the edit
  before it is made, so there would be no preview and no all-or-nothing check.
- **Open every changed file in the editor and apply it there:** simple undo, but a project-wide
  rename would open dozens of tabs.
- **Apply file by file and report the failures:** leaves the project half renamed, which does not
  build; the whole refactoring is refused instead.
- **A navigation lookup kind for rename:** a rename has a plan and a journal that outlive one
  request, so it has its own state.
