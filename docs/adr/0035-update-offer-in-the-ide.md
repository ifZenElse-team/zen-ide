# ADR-0035: The IDE offers and installs its own updates

- **Status:** Accepted
- **Date:** 2026-10-16
- **Extends:** [ADR-0034](0034-restart-after-update.md)

## Context

The owner (2026-10-16): when a new version is released while the IDE is open, a notification must
say so. And the restart after an update through Zen Toolbox did not happen: the bundle watcher of
ADR-0034 ran inside the window's frames, and a hidden or covered window (another full-screen Space,
another application in front) draws no frames.

## Decision

1. The bundle watcher runs on a thread of its own, every two seconds, independent of drawing. On a
   new version it saves every changed document and restarts as ADR-0034 describes.
2. An installed IDE (one running from its bundle) checks the release server at start and every 30
   minutes, with Zen Toolbox's own state and commands (`zen_toolbox::Tools`,
   `toolbox.releases.refresh`), on the same tools folder and applications folder as Toolbox. When a
   newer version of the IDE is out, a notification in the bottom right corner offers
   **Update and Restart**, **What's New** (the release notes since the installed version) and
   **Later** (until the next version). Updating runs `toolbox.tool.update`; the bundle is replaced
   and the watcher restarts the IDE. Progress and failures show in the notification.
3. Agents use `get_update_status` and `update_ide` (ADR-0033).

## Consequences

- Toolbox and the IDEs never disagree about what is installed: both read and write the same folder.
- Development builds (not in a bundle) neither watch nor check.
- An IDE older than this decision restarts only when its window is drawn; the first update to the
  version with this change may need a manual restart.
