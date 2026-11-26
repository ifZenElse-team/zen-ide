# ADR-0014: Refined layer model: the core gets its own layer

- **Status:** Accepted; the layer of `zen-update` amended by [ADR-0021](0021-zen-update-is-a-service.md); `zen-core` in L3 and `zen-modules` in L4 by [ADR-0071](0071-built-in-modules.md)
- **Date:** 2026-10-16
- **Amends:** [ADR-0004](0004-monorepo-and-layering.md) (layer numbering and the layer named in rules 3 and 4)

## Context

ADR-0004 placed `zen-core` in layer L4 together with the crates that build on it: `zen-plugins`,
`zen-jb-import`, `zen-update`, `zen-toolchain` and the language modules. Under the rule "a crate may
depend only on crates in lower layers", those crates could not depend on `zen-core`, yet each of
them registers commands in it and implements its extension points (ADR-0007). The problem surfaced
while implementing `cargo xtask layers`.

## Decision

The layers are:

| Layer | Crates |
|---|---|
| L0 foundation | `zen-text`, `zen-vfs` |
| L1 | `zen-syntax`, `zen-project` |
| L2 services | `zen-index`, `zen-lsp`, `zen-dap`, `zen-vcs`, `zen-terminal` |
| L3 | `zen-editor` |
| L4 core | `zen-core` |
| L5 features | `zen-plugins`, `zen-jb-import`, `zen-update`, `zen-toolchain`, and every language module `zen-lang-*` |
| L6 clients | `zen-ui`, `zen-automation` |
| apps | every crate in `apps/` |

The rules of ADR-0004 stay, restated with the new numbering and made precise:

1. A platform crate depends only on crates in strictly lower layers.
2. No platform crate depends on a language module. A language module depends only on platform crates
   in layers L0 to L4, and not on another language module (an exception needs an ADR).
3. Nothing below L6 depends on a UI toolkit or windowing crate (`egui`, `eframe`, `egui-wgpu`,
   `egui-winit`, `egui_kittest`, `wgpu`, `winit`, `rfd`).
4. Everything below L6 compiles for `wasm32-unknown-unknown`.
5. Applications may depend on any platform crate and language module, but not on another application.
6. `zen-testkit` may be used only as a dev-dependency. `xtask` is a development tool outside the
   layers; nothing depends on it.
7. Every crate lives in the directory of its kind (`crates/platform/`, `crates/lang/`, `apps/`) and is
   registered in `xtask/src/layers.rs`; an unregistered crate fails the check.

The registry in `xtask/src/layers.rs` is the source of truth for layer membership.

## Consequences

- `docs/architecture.md` and `crates/platform/README.md` use the new numbering.
- Rules apply to normal, build and dev-dependencies alike, so tests cannot hide a layering violation.

## Alternatives considered

- **Allow dependencies within a layer:** fewer layers, but cycles become possible and the order of
  crates in a layer becomes implicit.
- **Move command registration out of `zen-core` into each client:** contradicts ADR-0007.
