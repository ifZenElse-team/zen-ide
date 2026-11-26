# ADR-0008: Zen Toolbox and signed updates

- **Status:** Accepted; decision 4 (timing) amended by [ADR-0015](0015-toolbox-first.md); rollback removed for now by [ADR-0029](0029-no-rollback-for-now.md)
- **Date:** 2026-10-16

## Context

With several IDEs, users need one place to install, update, roll back and launch them. Updates must
be trustworthy, and installed users must never be stranded by a change of format.

## Decision

1. **Zen Toolbox** (`apps/zen-toolbox`) installs, updates, rolls back and uninstalls Zen IDEs,
   supports release channels (stable and nightly) and side-by-side versions, lists recent projects
   across all IDEs, creates shell launchers (`ronin`, `garden`, ...) and runs in the system tray.
2. It also installs and updates **language servers and toolchains**
   (rust-analyzer, gopls, ...) through `zen-toolchain`, and shares settings (theme, keymap, imports)
   across IDEs. It has no accounts and no licence management.
3. Releases are described by a **signed manifest** (ed25519) listing channels, versions and
   artifacts with their hashes. `zen-update` verifies signatures and hashes, installs atomically and
   keeps the previous version for rollback. IDEs installed without Toolbox use the same crate to
   update themselves, unless installed by a system package manager.
4. The manifest format, the signing key procedure and `zen-update` ship with the **first public
   release of RustRonin**. The Toolbox application itself is built when the second IDE appears.

## Consequences

- The release pipeline (`cargo xtask release`) must produce and sign the manifest.
- The signing key needs a documented custody and rotation procedure before the first release.

## Alternatives considered

- **Rely on system package managers only:** no single experience across operating systems, no
  side-by-side versions, no rollback.
