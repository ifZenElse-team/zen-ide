# ADR-0022: Zen Toolbox updates itself

- **Status:** Accepted; the kept copy (decision 2) removed by [ADR-0028](0028-no-self-update-backup.md)
- **Date:** 2026-10-16

## Context

Zen Toolbox installs and updates the IDEs (ADR-0008) and is the entry point to the family
(ADR-0018). The owner decided that it must also keep itself up to date.

## Decision

1. Zen Toolbox is a product in the release manifest with the id **`toolbox`**. Its artifact is a
   `.tar.gz` containing `bin/zen-toolbox` (`.exe` on Windows), like every other tool.
2. When the manifest has a newer `toolbox` release for the channel and platform than the running
   build, Toolbox shows it above the tools list. Updating downloads the artifact, verifies it
   against the signed manifest, and replaces the running executable atomically; the replaced file is
   kept next to it as `<executable>.previous` for rollback.
3. The update takes effect after a restart, which the user starts from the same place ("Restart").
   Toolbox restarts itself with the same arguments.
4. A build's version is the crate version unless the release pipeline sets `ZEN_BUILD_VERSION`.

## Consequences

- On macOS the release artifact will later be an application bundle; the replacement step then
  swaps the bundle instead of a single file. The manifest format does not change.
- If Toolbox is installed in a location the user cannot write to, self-update fails with a clear
  message; installing Toolbox per user (the default) avoids this.

## Alternatives considered

- **A separate updater process:** needed only where a running executable cannot be renamed;
  Windows, macOS and Linux all allow renaming it.
