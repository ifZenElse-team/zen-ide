# ADR-0021: zen-update is a service in layer L2

- **Status:** Accepted
- **Date:** 2026-10-16
- **Amends:** [ADR-0014](0014-refined-layer-model.md) (the layer of `zen-update`)

## Context

ADR-0014 put `zen-update` in L5, among features that register commands. As built (ADR-0020), it is a
library: the release manifest, signatures, downloads and installations, with no commands and no
dependency on `zen-core`. The commands that use it live in features: `zen-toolbox` now, and each
IDE's self-update later. `cargo xtask layers` rejected `zen-toolbox` (L5) depending on `zen-update`
(L5).

## Decision

`zen-update` belongs to layer **L2, services**, next to `zen-lsp`, `zen-dap` and `zen-vcs`. It keeps
depending on no workspace crate. Features in L5 use it and expose its functions as commands.

## Consequences

- The registry in `xtask/src/layers.rs` and `docs/architecture.md` list `zen-update` in L2.
- `zen-update` must stay free of commands and UI; a command that installs or updates belongs to the
  feature that offers it.

## Alternatives considered

- **Allow L5 crates to depend on each other:** reopens the problem ADR-0014 solved.
- **Move the toolbox commands into zen-update:** mixes a reusable service with one product's
  feature.
