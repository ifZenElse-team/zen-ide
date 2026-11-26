# ADR-0222: Safe Delete and a refactoring dialog that providers fill

- **Status:** Proposed
- **Date:** 2026-11-04

## Context

The Refactor menu had Rename, Extract/Introduce and Inline. It lacked Safe Delete
(⌘⌦): a declaration or a file is deleted only when nothing else uses it, and otherwise a dialog
lists the usages with View Usages, Delete Anyway and Cancel. Other refactorings that need
input (Move, Change Signature in Java) share one dialog shape: the refactoring's fields, Preview,
Refactor, and the conflicts found before anything changes. The Java and Kotlin plug-ins will add
such refactorings through their language servers; the dialog must be the platform's, and the
plug-ins must be able to fill it without a dialog of their own (ADR-0050).

No language server we run offers Safe Delete. Every one offers `textDocument/documentSymbol` and
`textDocument/references`.

## Decision

1. **A refactoring is a provider.** `RefactoringProvider` (in `zen-workbench`) has four steps: the
   form (title, fields of kind text, check box or choice with their values to start from, and the
   labels of the conflict list), the plan with the fields' values (text changes by file, file
   operations, conflicts, or why not), the change, and cancel. Providers are kept in the session
   (`RefactoringProviders`); Safe Delete and Rename are built in, and a plug-in's code adds its own.
   The commands `refactor.dialog.list`, `.form`, `.preview`, `.apply` and `.cancel` take the
   provider's id and its `context`, so agents run any refactoring the way the dialog does.
   `refactor.dialog.apply` computes the plan first and refuses while it has conflicts unless
   `force`.
2. **One dialog in the window.** It shows the form; Preview opens the existing Refactoring Preview
   tab of the Find tool window, whose Do Refactor applies with `force`; Refactor computes the plan
   and, with conflicts, shows them with the provider's labels (Continue or Delete Anyway, Show
   Conflicts in View or View Usages, Cancel). Show in View lists the conflicts that have places on
   the Refactoring Preview tab. A form marked `immediate` with no fields is not shown: without
   conflicts the refactoring is made at once.
3. **Safe Delete finds the declaration with document symbols and its usages with references.** The
   declaration is the innermost document symbol whose name starts at or holds the caret; for a file
   (or a folder's files), every top-level symbol and the methods declared outside their type's body.
   References are asked without the declaration; usages inside the declaration's deleted text or
   inside the deleted files are left out. A file no running server reads has no usages.
4. **What a declaration's deletion removes:** the server's range when it starts and ends its lines
   (a `;` may follow), widened to whole lines, up over the lines directly above that start with
   `@`, `#[` or `//` and over a `/* */` block ending right above, and down over one blank line when
   a blank line or the file's start is above (at the file's end, over the blank line above instead). A declaration that shares its line with other code is
   cut exactly. The change goes through the workspace-edit path (one journal entry, so one Undo).
5. **Files go through the Project tool window's delete**, `file.fs.delete`: the Trash, the servers'
   `willDeleteFiles` edits and `didDeleteFiles` (ADR-0215).
6. **⌘⌦ is Safe Delete** everywhere: in the editor it takes the caret's declaration, in the Project
   tool window its selection (where ⌫ and ⌦ stay Delete).

## Consequences

- Safe Delete is as good as the server's references: an overriding method, a reflective use or a
  use in a file the server does not read is not found. An option to search comments
  and strings is not there.
- A plug-in refactoring (Move and Change Signature in Java) implements the provider
  and gets the dialog, the preview, the conflict list, the commands and the undo step for nothing.
- Rename keeps its own dialog in the window; its provider proves the shared one with a text field
  and is what agents get from `refactor.dialog.*`.
