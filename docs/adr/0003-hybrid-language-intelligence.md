# ADR-0003: Hybrid language intelligence

- **Status:** Accepted
- **Date:** 2026-10-16

## Context

An IDE's value lies in how well it understands code. Building our own semantic model for every
language would take years per language. Language servers (rust-analyzer, gopls,
pyright, ...) already provide completion, navigation, refactoring and diagnostics through the
Language Server Protocol, but they are external processes with latency, and their feature set is
fixed by the protocol.

## Decision

Language intelligence has two layers:

1. **Syntax layer, owned by the platform** (`zen-syntax`): incremental tree-sitter parsing for
   highlighting, folding, the structure view, structural selection, indentation and syntax-level
   inspections. It is synchronous with typing and never waits for a language server.
2. **Semantic layer, from the language server** (`zen-lsp`): completion, go to definition, find
   usages, rename and refactorings, diagnostics, inlay hints, code actions.

The platform's own analysis grows over time where it adds clear value. For Rust, rust-analyzer's
library crates (`ra_ap_*`) may later be embedded in-process; that is an implementation change
behind the same extension points, not a change of architecture.

## Consequences

- RustRonin gets professional-level semantic features from the first milestone.
- Every new IDE starts with an existing language server and a tree-sitter grammar.
- Features beyond LSP (for example some refactorings) require either protocol
  extensions, in-process integration, or our own analysis, decided per feature.
- Language servers are external programs to install and keep updated; `zen-toolchain` and
  Zen Toolbox handle this.

## Alternatives considered

- **LSP only:** simplest, but highlighting and structure depend on server latency, and the
  experience is capped by the protocol.
- **Our own semantic analysis per language:** the highest ceiling, but years of work
  per language before the first useful release.
