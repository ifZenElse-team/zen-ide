# ADR-0219: Extract and Inline through code actions, named in place

- **Status:** Proposed
- **Date:** 2026-11-04

## Context

The Refactor menu had only Rename. It also needs Extract/Introduce (Variable,
Constant, Field, Parameter, Method) and Inline, with their own shortcuts, a chooser when several
expressions qualify at the caret, and the new name edited in place at every occurrence. Every
language server we run offers these as code actions, but each names them its own way: kinds such
as `refactor.extract.variable` (JDT LS, gopls), `refactor.introduce.parameter` (JDT LS), a bare
`refactor.extract` with the title saying which (rust-analyzer), or `refactor.extract.constant` for
a local, a module constant and a class field alike (the TypeScript server). Some send the edit,
some a command that sends it back through `workspace/applyEdit`; few say what the new name is.

## Decision

1. **One command per refactoring, in the platform**: `refactor.extract.variable`, `.constant`,
   `.field`, `.parameter`, `.method` and `refactor.inline.apply`. Each asks the server of the file
   for code actions with `context.only` (`refactor.extract` and `refactor.introduce`, or
   `refactor.inline`) and keeps those that fit, by kind, and by title only where the kind does not
   say (a bare `refactor.extract`, TypeScript's constant kind: "enclosing scope" or "in function"
   is a variable, "field" a field). Applying goes through the context actions' path: resolve,
   edit, command and the edits it sends, all in one journal entry, so one Undo reverts it.
2. **The expressions at the caret** come from the editor's own syntax units joined with the
   server's selection ranges, as Extend Selection does, kept when they are one line, not a whole
   statement, and balanced, without a top-level comma, semicolon or assignment. With several, the
   command lists them and changes nothing; the window shows the chooser. Several fitting actions
   (one occurrence or all, a scope or another) are listed the same way.
3. **The new name** is the placeholder of a snippet edit when the server sent one, else the place
   the server asks the client to rename at (the TypeScript server's `_typescript.rename` request,
   answered with `null`, or a client-side rename command, which is not sent back to the server);
   otherwise the identifier whose uses the change added, preferring one added at least twice (a
   declaration and a use), then one new to the file, then the nearest to the extracted code. Its
   occurrences are its whole-word matches in the file; when the name was there before, only the
   ones the change added, by a token diff of the changed part. The client declares
   `experimental.snippetTextEdit`, so rust-analyzer marks its name; every snippet edit, in any
   workspace edit, goes in as plain text (`$0`, `${1:x}` and escapes removed).
4. **In-place naming is multi-caret editing.** The window selects every occurrence and follows
   them while the user types; a list of suggested names (the server's, then the words of the
   expression's last name, in the name's style) sits under the caret. Enter takes the highlighted
   suggestion, Escape keeps the name, an edit or caret outside the occurrences ends it. Agents pass
   `name` instead, which renames the occurrences in the same journal entry.
5. **The shortcuts** are ⌥⌘V, ⌥⌘C, ⌥⌘F, ⌥⌘P, ⌥⌘M and ⌥⌘N, bound in the editor: outside it ⌥⌘N
   stays New Branch and ⌥⌘F the database filter.

## Consequences

- A server's new kind or title needs a line in `refactoring_of`; until then that action is only in
  Show Context Actions.
- Undo reverts the typed name first, then the refactoring, as two steps.
- Extract Field and Parameter are only as good as the server: gopls, rust-analyzer and the
  TypeScript server have no parameter extraction, and the message says so.
