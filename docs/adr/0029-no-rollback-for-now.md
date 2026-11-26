# ADR-0029: No rollback of IDEs for now

- **Status:** Accepted
- **Date:** 2026-10-16
- **Amends:** [ADR-0008](0008-toolbox-and-updates.md) (decision 3, rollback), [ADR-0025](0025-macos-applications.md) (decision 2), [ADR-0028](0028-no-self-update-backup.md) (decision 3)

## Context

Zen Toolbox kept the previous version of every installed IDE in its tools directory so that the ⋮
menu could roll back. The owner decided that keeping old versions on disk is not worth it as
designed, and that rollback will come back with a different mechanism.

## Decision

1. Zen Toolbox keeps **only the active version** of each tool. Installing or updating deletes every
   other version; the `previous` pointer is gone.
2. The `toolbox.tool.rollBack` command and the "Roll back" menu item are removed.
3. At start-up Toolbox prunes what older versions kept for rollback.
4. A new rollback mechanism is designed separately and recorded in its own ADR. Until then, going
   back means publishing a fixed release (or withdrawing a bad one, ADR-0026).

## Consequences

- Installed tools take the disk space of one version.
- A bad release affects users until a newer one is published.
