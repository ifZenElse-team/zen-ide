# ADR-0229: Java Move and Change Signature through JDT LS in the refactoring dialog

- **Status:** Proposed
- **Date:** 2026-11-05

## Context

JvmJizo's Refactor menu had Rename, Extract/Introduce, Inline and Safe Delete. Java users expect
**Move…** (F6) for classes and packages and **Change Signature…** (⌘F6) for methods. ADR-0222 gave
refactorings a shared dialog filled by a `RefactoringProvider`, and ADR-0215 made language server
edits create, rename and delete files.

Eclipse JDT LS answers these requests, checked against the server JvmJizo installs (1.63.0):

- `java/getMoveDestinations` with `{ moveKind: "moveResource", sourceUris, params }` lists the
  packages (`displayName`, `uri`, `project`, `isDefaultPackage`, `isParentOfSelectedFile`), and
  `java/move` with one of them as `destination` and `updateReferences` answers
  `{ edit, errorMessage? }`: the `package` line, the imports of the code that uses the class, and
  the file's rename. It refuses a package folder as a source.
- `workspace/willRenameFiles` for a package folder (its filters take folders) answers the edits a
  package rename needs: the `package` lines and the imports.
- `java/getChangeSignatureInfo` with code action parameters on a method's name answers
  `{ methodIdentifier, modifier, returnType, methodName, parameters: [{ type, name, defaultValue,
  originalIndex }], exceptions }` (nothing on a call). `java/getRefactorEdit` with
  `command: "changeSignature"` and the arguments `[methodIdentifier, delegate, name, visibility,
  returnType, parameters, exceptions, false]` answers the edit; with seven arguments, a new
  parameter without a default value or two parameters of the same name it answers nothing. Its edit
  is one change over everything from the declaration to the last call in a file.

None of these needs an `extendedClientCapabilities` flag. The code actions the server offers for
them (`refactor.move`, `refactor.change.signature`) run a client command
(`java.action.applyRefactoringCommand`) that only its own editor extension understands.

A plug-in is WebAssembly and cannot hold a provider trait object; the requests and their answers
are specific to JDT LS.

## Decision

1. **Two providers in the workbench's Java code**, `crates/platform/workbench/src/java/refactoring.rs`,
   beside the other code that speaks JDT LS (class files, the debugger): `javaMove` and
   `javaChangeSignature`. They accept `.java` files a running language server serves. A generic
   provider fed by a server declaration would need the request shapes, the argument order and the
   answers' meaning declared as data, which is more than the code it replaces and serves one server.
2. **A provider says which window action it answers** (`action()`: `move`, `changeSignature`) and
   whether it accepts a place (`accepts()`), both defaulting to none. **F6** (`view.project.move`)
   and **⌘F6** (`view.changeSignature.open`) open the dialog with the provider that answers the
   editor's file and caret or the Project tool window's selection; F6 on a scratch file keeps its
   own Move dialog. `refactor.dialog.list` gives each provider's `action`.
3. **Move of classes** asks `java/getMoveDestinations` for the form (one choice, the destination's
   folder URI, starting with the class's own package) and `java/move` for the plan. Every top-level
   class without `public` has its references asked; those left in the old package are conflicts.
   Moving onto a file that exists is refused before the server is asked.
4. **Move of a package** is a folder rename: the plan asks the server's `workspace/willRenameFiles`
   for the new folder (the destination package's folder joined with the package's last name), adds
   the rename, and the change sends `workspace/didRenameFiles`. Moving a package into itself is not
   offered.
5. **Change Signature** asks `java/getChangeSignatureInfo` at the caret and, when that is a call,
   at `textDocument/definition`'s answer. The form has `visibility`, `returnType`, `name`, a
   `parameters` table and `delegate`; the plan checks the names and that every new parameter has a
   default value, lists a removed parameter the body still uses (found with the Java scanner) as a
   conflict, and sends `java/getRefactorEdit`. The server's edit is split into its changed lines so
   that the preview shows the declaration and each call apart. The method's exceptions are sent
   back unchanged.
6. **The dialog gets a table field** (`kind: table`, `columns`, `newRow`): rows of strings by column
   id, with Add, Remove, Move Up and Move Down; keys the provider put in a row (such as
   `originalIndex`) stay in it.

## Consequences

- Both refactorings are one undo step and one Local History action each (`move`,
  `changeSignature`), through the same workspace-edit path as Rename.
- Move has no search in comments, strings or text occurrences, no move to a folder that is not a
  package, and no move of inner classes or members. Change Signature has no Exceptions tab, no
  signature preview and no propagation of new parameters to callers.
- A package-private member a moved class uses from its old package is not found; the server's
  edit is applied and the compiler reports it.
- Kotlin, and any other language, can add its providers the same way; the window actions find
  them by `action`.
