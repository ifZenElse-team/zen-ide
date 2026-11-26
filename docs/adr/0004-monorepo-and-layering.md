# ADR-0004: A single monorepo with enforced layering

- **Status:** Accepted; layer numbering amended by [ADR-0014](0014-refined-layer-model.md)
- **Date:** 2026-10-16

## Context

All Zen IDEs share about ninety percent of their code: editor, project model, search, version
control, terminal, debugger and UI. The platform API will change often in the first years. One
repository per app with shared libraries pinned by git revision suits apps that share little; for us it would turn every platform change into a chain of cross-repository
updates.

## Decision

Zen IDE lives in one repository and one Cargo workspace (`zen-ide`): platform crates in
`crates/platform/`, language modules in `crates/lang/`, applications (every IDE, Zen Toolbox and
`zen-cli`) in `apps/`, and build automation in `xtask/`.

Layering rules, enforced by `cargo xtask layers` in CI:

1. Each platform crate belongs to a layer (L0 to L5, see `docs/architecture.md`) and may depend
   only on crates in lower layers.
2. No platform crate depends on a language module.
3. Nothing below L5 depends on a UI toolkit or windowing crate (egui, eframe, wgpu, winit, rfd).
4. Everything below L5 compiles for `wasm32-unknown-unknown`.
5. Applications contain branding, packaging and the list of bundled language modules; no feature
   logic.

## Consequences

- A platform change and its effect on every IDE are reviewed and tested in one change.
- CI must stay fast as the workspace grows (incremental caching, test partitioning).
- Projects with an independent life cycle (the website, a future plug-in SDK) may live in separate
  repositories.

## Alternatives considered

- **One repository per IDE plus a platform repository:** independent release cycles, but constant
  version-bump friction while the platform API is unstable.
