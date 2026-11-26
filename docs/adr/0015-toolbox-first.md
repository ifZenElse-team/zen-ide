# ADR-0015: Build Zen Toolbox first

- **Status:** Superseded by [ADR-0280](0280-clean-room-process.md)
- **Date:** 2026-10-16
- **Amends:** [ADR-0008](0008-toolbox-and-updates.md), decision 4 (when Toolbox is built)

## Context

ADR-0008 planned to build the Zen Toolbox application when the second IDE appears, after the first
public release of RustRonin. The owner decided to start with Toolbox instead. Three things make this
a good first application:

- **It validates the UI stack on a small surface.** Toolbox is a single window with lists, buttons,
  menus, a settings page and a tray icon. Building it tests whether egui can deliver the look and
  behaviour we specify before the much larger editor is built on it (ADR-0006).
- **It forces the delivery infrastructure early.** Release manifests, signatures, atomic
  installation, updates and rollback (`zen-update`) are needed for every release anyway.
- **It exercises the platform's core.** Toolbox follows "everything is a command" (ADR-0007), so
  `zen-core` is built against a real client from the start.

## Decision

1. **Zen Toolbox is the first application.** The roadmap phase after the platform skeleton is
   "Zen Toolbox"; the editor and RustRonin follow.
2. *(Withdrawn by [ADR-0280](0280-clean-room-process.md): this decision named the visual reference
   for the application.)*
3. **Release source during development:** a local manifest (`file://`), signed with a development
   key that is never used for real releases. Nothing is published until the project goes public.
4. **Scope of the first version:** the list of available and installed tools, installation,
   updates and rollback from the local manifest, release channels, the projects list, settings, and
   the tray icon. Toolchain installation (`zen-toolchain`) and settings sync come later.

All other decisions of ADR-0008 stand.

## Consequences

- `zen-core` (minimal command registry), `zen-update` and the first `zen-ui` components are built
  before the editor crates.
- Until an IDE exists, Toolbox lists RustRonin and the other products as not yet available, and is
  tested with a fake product in the local manifest.
- The UI specification method (docs/behaviour.md) is first applied to Toolbox.

## Alternatives considered

- **Editor first, as planned:** delivers the product users care about sooner, but risks discovering
  UI stack limits late, after a large investment.
