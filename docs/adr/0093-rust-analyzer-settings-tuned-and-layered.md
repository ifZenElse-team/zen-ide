# ADR-0093: rust-analyzer's settings are tuned for large workspaces, and the user and the project can change them

- **Status:** Proposed
- **Date:** 2026-10-22
- **Relates to:** [ADR-0055](0055-language-servers.md), [ADR-0088](0088-module-toolchains-side-by-side.md),
  [ADR-0071](0071-built-in-modules.md)

## Context

The team develops this repository (70 packages) through a headless RustRonin.
rust-analyzer started with fixed options from the Rust plug-in's
`language-servers.json` (`cargo.targetDir`, `checkOnSave`, `check.command`); neither the user nor
a project could change them, and they had never been measured. The baseline on this repository
(docs/performance.md, "rust-analyzer on a large workspace") showed:

- every save ran `cargo check --workspace --all-targets`, and so did loading the project: the
  first lookups after opening it waited for that check (Go to Symbol up to its 10 s limit,
  rename 6–8 s after a save), and a save's problems took 5–7 s;
- an edit's own problems never came after a lookup: rust-analyzer cancels pulled diagnostics
  (`ServerCancelled`) while it reloads files, the IDE took that as an answer, and
  `problems.file.list` said the file was checked and clean.

## Decision

1. **Measured defaults.** The plug-in's defaults are what was measured best on this repository,
   one change at a time: `check.workspace: false` (a save checks the saved file's package, and
   loading the project checks nothing), `files.exclude: [".claude/worktrees"]` (agents' copies
   of a repository inside it), and the existing `cargo.targetDir: true` (a folder of its own
   under `target`, never waiting for the user's builds). Cache priming, build scripts, procedural
   macros and the LRU capacity stay rust-analyzer's own: turning them off made loading faster but
   the first lookups slower or the analysis wrong (derives unexpanded), and a larger LRU changed
   nothing measurable.
2. **Three layers, rust-analyzer's own names.** rust-analyzer starts with the defaults, the
   user's settings over them (`toolchain-rust.json`, `rustAnalyzerSettings`) and the project's
   over both (`.zen/rust-analyzer.json` at its root, which the repository carries). Settings are
   rust-analyzer's, as its manual names them without the prefix, nested as JSON objects; objects
   merge key by key. Settings → Languages & Frameworks → Rust → Performance shows the settings
   that matter on a large workspace; `toolchain.rustAnalyzer.settings` tells every layer and
   `toolchain.rustAnalyzer.configure` sets one key for the user or the project, which restarts
   rust-analyzer (ADR-0007, ADR-0033).
3. **A platform hook.** `zen_lsp::ServerTools::initialization_options(server, root)` gives the
   options a declared server starts with in a project (the declared ones by default); the
   language's toolchain adds its layers. The workbench calls it on the thread that starts the
   server, never on the window's, and answers `workspace/configuration` from the same options, so
   a configuration change (inlay hints) keeps them.
4. **Cancelled pulls are asked again.** A pulled-diagnostics request the server cancels
   (`ServerCancelled`, `ContentModified`) is asked again after 300 ms, and a file whose current
   text has no answer yet is "checking", not checked.

## Consequences

- A save no longer reports what a change breaks in other packages until a file there is saved
  (their open files show rust-analyzer's own analysis at once); the page's "What a save checks →
  The whole workspace", or a project's `.zen/rust-analyzer.json`, brings the workspace check back.
- A hand edit of `.zen/rust-analyzer.json` applies when rust-analyzer restarts; the command
  restarts it.
- Another language's toolchain can layer its server's settings the same way.
