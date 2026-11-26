# ADR-0006: Desktop first, egui on wgpu, web as a thin client

- **Status:** Accepted; the Phase 2 spike concluded by [ADR-0054](0054-egui-spike-concluded.md)
- **Date:** 2026-10-16

## Context

The IDEs must feel native and fast on macOS, Windows and Linux, and the project is pure Rust. A
browser version is attractive, but an IDE needs to start processes (compilers, language servers,
debuggers), use a real file system and run a terminal, none of which a browser provides.

## Decision

1. The desktop application is the primary product.
2. The UI is built with **egui** on **wgpu**. The UI layer is
   thin: it renders state and dispatches commands. Should egui stop meeting our needs (for example
   in text rendering or accessibility), the layering keeps a replacement possible.
3. Everything below the UI layer compiles to WebAssembly from the start, to keep the core
   independent of the operating system.
4. A browser client comes later as a **thin client**: the UI runs in the browser, while files,
   builds and language servers run on a server or in a container.

## Consequences

- Text rendering, editor performance with large files and accessibility in egui must be validated
  early (Phase 2 includes a performance and accessibility spike).
- No web view and no JavaScript in the product.

## Alternatives considered

- **Electron or Tauri with a web UI:** familiar tooling, but contradicts native performance and
  pure Rust.
- **Native toolkits per operating system:** best platform fidelity, but three UIs to maintain.
- **Other Rust UI toolkits (Slint, Iced, GPUI):** viable; egui was chosen for maturity, WebAssembly
  support and the experience available in sibling projects. GPUI remains a reference point for
  editor rendering performance.
