# ADR-0088: Built-in modules keep their toolchains side by side; diagnostics are pulled too

- **Status:** Proposed
- **Date:** 2026-10-21
- **Amends:** [ADR-0071](0071-built-in-modules.md) (decision 2: "the first module that gives a
  toolchain is the IDE's")
- **Relates to:** [ADR-0055](0055-language-servers.md), [ADR-0046](0046-project-models-and-outlines.md),
  [ADR-0003](0003-hybrid-language-intelligence.md)

## Context

RustRonin gets rust-analyzer, Cargo's project model and Cargo's runs; the owner develops
this repository through RustRonin's headless MCP next. Until now
only Go gave a toolchain (`LanguageToolchain`), and `ModuleHost::toolchain` kept the first one
given: with Rust and Go both on, one module would find, start and install the other's language
servers with its own tools, and RustRonin with Go switched on (ADR-0071, "Consequences") would
lose Go's SDK.

rust-analyzer also showed two gaps of the language server layer:

- It serves its own analysis (type errors, unresolved names) only as **pulled diagnostics**
  (`textDocument/diagnostic`, LSP 3.17) and publishes only what `cargo check` finds; a client
  that does not pull sees no problem until the file is saved. What it published for a file opened
  before the project was loaded (every `#[test]` "unresolved") was never published again.
- It works for a long time after starting on a large workspace (loading, indexing, checking) and
  says so with `$/progress` and `experimental/serverStatus`; agents had no way to know when its
  answers are complete.

## Decision

1. **Every module's toolchain is kept.** `ModuleHost::toolchain` collects them; the IDE's
   toolchain is `zen_modules::Toolchains`, which gives each language server to the toolchain that
   serves it (`LanguageToolchain::serves`: its languages by default; Go also takes servers
   installed with `go install`, Rust those installed with rustup or `cargo install`) and the
   others to the system's tools. Runs, project models and debugging take the first toolchain that
   gives them something; a module that needs its own (Cargo) asks its toolchain itself.
   `LanguageToolchain::generations` lets the window restart only the languages whose toolchain
   changed, and the project model is read again after any change.
2. **Diagnostics are pulled from a server that offers them**, once per version of each open file
   and again when the server asks (`workspace/diagnostic/refresh`); they are shown with the
   published ones, each once. A server that asks for a refresh but only publishes gets its open
   files opened again (`Client::reopen`), and publishes them anew.
3. **What a server is busy with is known.** `$/progress` and `experimental/serverStatus` (which
   the client now asks for) make `lsp.servers.list` give `busy`, `progress` and `readyAfterMs`,
   and a server's warning (`health` not `ok`) as its `message`; the status bar shows the work, e.g.
   "rust-analyzer: Indexing 18%".
4. **rustup installs servers**: `InstallKind::Rustup` (`rustup component add <package>`), for
   rust-analyzer.

Choosing a toolchain is the person's: the commands that add, remove or choose a Go SDK, gopls, Delve,
a Rust toolchain or rust-analyzer refuse an agent (`agentNotAllowed`) before they start the folder's
program to read its version, as does a rust-analyzer setting that names a program. Listings and the
IDE's own installs stay open.

## Consequences

- RustRonin with Go switched on, and GoGarden with Rust switched on, have both languages' servers,
  each with its own tools; a Go SDK change restarts gopls only.
- Problems of Rust code appear as you type (about 0.25 s after an edit on this repository) and
  `cargo check`'s after a save; nothing stale stays after the project is loaded.
- An agent waits for `busy: false` before trusting usages or problems, instead of guessing.

## Alternatives considered

- **One toolchain per IDE, chosen by the product:** breaks "switch Go on in RustRonin" (ADR-0071).
- **Only reopening files on refresh, no pulling:** rust-analyzer then shows its own analysis only
  when a file is opened or saved, never while typing.
- **rust-analyzer's `checkOnSave` only:** problems would wait for a save and a `cargo check`,
  seconds to minutes on a large workspace.
