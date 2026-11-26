# ADR-0256: Inspect Code over a scope

- **Status:** Accepted
- **Date:** 2026-11-06
- **Relates to:** [ADR-0059](0059-go-mod-dependency-checks.md) and [ADR-0066](0066-background-tasks-and-go-mod-checks.md)
  (extended, not replaced), [ADR-0033](0033-ai-access-to-every-api.md),
  [ADR-0050](0050-products-are-plugin-bundles.md)

## Context

Inspections reached only the open files, and language servers only what they chose to report. The
requirement: Inspect Code over a scope with an Inspection Results view, a run over a chosen
scope, with results reviewed in a tree. How the results group by directory and what the view
remembers are still open.

## Decision

1. **A run is a background task of the platform** (`zen_workbench::inspect_code`,
   `inspect.code.run`). The scope is the whole project, a directory, chosen files, the files with
   uncommitted changes or any scope of `search.scope.list`, narrowed by Include test sources (the
   scopes' own idea of a test file) and a file mask. A run cancelled with
   `tasks.background.cancel` keeps no results; one run goes at a time, and a new one cancels it.
2. **Inspectors run on a thread of their own.** Every `Inspector` and plug-in `TextFeatures`
   handling a file in scope gets its text (the open document's, or the file on disk). An inspector
   looks things up in the background as ADR-0059 says, so a first run over a `go.mod` shows what
   is known by then.
3. **Language servers get files by lending, not by a pull of the whole workspace.** The servers we
   ship do not all offer `workspace/diagnostic`, and a pull says nothing about servers that only
   push, so the run opens files for the server for a moment, as a lookup in a closed file does
   (`language::sweep`): at most sixteen at a time, each until its diagnostics came. A server may
   report in stages: gopls first says what the compiler says and adds its analyzers a moment later
   (seen on a real project on 2026-11-06), so a server can declare `diagnosticsSettleMs`, how long
   a file stays open after that, 2000 for gopls. A file with no answer in 20 seconds counts as not checked, and after the first such file a server gets three
   seconds per file, so a server that never reports an empty file cannot hold a project for
   hours; the clock stops while the server reports work under way. The results list the files no
   server checked and why. With Power Save Mode on no server is asked, and the results say so.
4. **What is only for open files stays so.** Spelling needs the editor's highlighting and
   declarations of a document, so it covers the open files in scope. A run reads the rest of an open
   file's problems (database consoles, injected SQL) as the Problems tool window does.
5. **An inspection is `source/code`.** A finding carries both, a language server's diagnostic may
   carry either; the id of its inspection joins them, and asking for an inspection by name matches
   the id, the source or the code. Inspectors declare what they report
   (`Inspector::inspections`), and servers declare it in their declaration (`inspections`, with the
   texts in English and Russian); Run Inspection by Name also offers what results have shown.
6. **Results live in the workbench** (`InspectCode`), up to eight runs, a rerun replacing its own.
   The tree (severity, inspection, file, with directories on request) is a command, so an agent
   reads what the window shows. Offsets are those of the run: after editing, Rerun.
7. **Fixes.** An inspector's fixes run through the same jobs as `problems.fix.apply`, one job per
   file. A language server's fix is a context action found by what the problem says rather than by
   offset, because a fix before it may move it; a queue makes them one at a time, waiting for the
   server's new diagnostics and `diagnosticsSettleMs` in between (gopls offers some fixes only after
   its analyzers ran, so a fix takes it at least that long), and each is undone like any context
   action. A skipped fix says why in the task's log.
8. **Export** writes HTML to read and XML of our own (`<problems>` of `<problem>` with file, line,
   column, severity, inspection and description), with no excluded items.

## Consequences

- A whole-project run opens every file a server serves, a few at a time, in that server; a server
  that is slow to report makes a run long, and the task can be cancelled.
- There is no offline command-line inspection script yet: `inspect.code.run` over MCP on a
  headless IDE does that job for agents.
- `workspace/diagnostic` can be added later for servers that offer it, without changing the results.
