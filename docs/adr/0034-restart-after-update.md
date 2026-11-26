# ADR-0034: An IDE restarts into its new version after an update

- **Status:** Accepted
- **Date:** 2026-10-16

## Context

The owner (2026-10-16): when Zen Toolbox updates an application, the application must restart into
the new version by itself. Toolbox replaces the whole `.app` bundle (ADR-0025); a running IDE keeps
running the old code until it is restarted.

## Decision

1. A running IDE watches its own bundle: every two seconds it reads `CFBundleShortVersionString`
   from `Contents/Info.plist` and compares it with the version it was built as. Development builds
   (not in a bundle) do not watch.
2. When the version differs, the IDE saves every changed document in every project,
   keeps the open projects marked open, starts a small shell
   that waits for the process to end and opens the same bundle again, and exits. The new version
   reopens the projects.
3. Toolbox needs no change: replacing the bundle is the signal. Zen Toolbox itself already restarts
   after its own update.

## Consequences

- Unsaved edits are saved, not lost; terminal sessions end with the old process.
- Versions before GoGarden 0.3.5 do not watch, so the first update to 0.3.5 still needs a manual
  restart.
- The bridge (`<ide> mcp`) and agents see the IDE disappear for a moment and reconnect.
