# ADR-0167: Java hot code replace through java-debug

- **Status:** Proposed
- **Date:** 2026-10-26
- **Builds on:** [ADR-0153](0153-java-debugging-and-tests.md) (Java debugging with java-debug),
  [ADR-0052](0052-debugger-zen-dap-and-delve-over-reverse-dap.md)

## Context

The IDE should reload changed classes into a program being debugged: after a build, and on **Run →
Debugging Actions → Reload Changed Classes**. java-debug, the debug adapter Eclipse JDT LS runs,
does the same through two extensions of the Debug Adapter Protocol: a `hotcodereplace` event whose
`changeType` is `BUILD_COMPLETE` once the server has built changed classes, and a custom request,
`redefineClasses`, that replaces in the JVM the classes built since the last time and answers
`{ changedClasses, errorMessage? }`. The server builds on `textDocument/didSave`.

## Decision

1. An adapter a language server runs says it reloads classes (`ServerAdapter::hot_code_replace`,
   set for java-debug). Other adapters do not, and reloading there is refused with a message.
2. **After a build**: when java-debug reports `BUILD_COMPLETE`, the session sends `redefineClasses`
   by itself. **On request**:
   `debug.classes.reload` (Run → Reload Changed Classes, no default shortcut)
   does the same for the current session or a given one.
3. The outcome is a line in the session's console and a notification of the Debugger group: the
   classes replaced, nothing to replace, or why it failed (java-debug's message, e.g. a change the JVM
   cannot apply). The command answers it, and `debug.session.get` gives the last one (`reloaded`);
   a reload done by itself is notified when the window next reads the session.
4. The workbench tells a language server of every save (`textDocument/didSave`), also of an edit and
   its save made between two syncs, as an agent's `editor.text.replace` with `save: true` does: the
   save count of a document, not only its dirty flag, decides.

## Consequences

- Saving a changed method body while debugging Java takes effect at its next call. A method on the
  stack keeps its old code until it returns; the JVM refuses structural changes (methods, fields,
  signatures), which the notification reports.
- The server builds the whole project after a change, so `changedClasses` may list every class it
  built since the program started, not only the edited one.
- Asking before reloading is not offered: reloading is what a save while
  debugging is for, and a failure leaves the program as it was.
