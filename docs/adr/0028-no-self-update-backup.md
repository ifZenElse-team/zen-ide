# ADR-0028: Zen Toolbox keeps no copy of itself after updating

- **Status:** Accepted
- **Date:** 2026-10-16
- **Amends:** [ADR-0022](0022-toolbox-updates-itself.md), decision 2 ("the replaced file is kept … for rollback")

## Context

Self-update kept the replaced Zen Toolbox next to the new one (`Zen Toolbox.app.previous`,
`zen-toolbox.previous`) for a rollback that the interface never offers. The owner asked not to keep
it: it wastes disk space for nothing.

## Decision

1. A self-update deletes the replaced Toolbox as soon as the new one is in place. If the swap fails,
   the old Toolbox is put back, so a failed update still leaves a working Toolbox.
2. At start-up Toolbox deletes leftovers of earlier versions (`*.previous`, `*.zen-old`, unfinished
   `*.zen-partial`).
3. Unchanged: IDEs keep **one** previous version in the tools directory, because rolling an IDE back
   is a feature (the ⋮ menu); older versions are pruned on every install.

## Consequences

- A broken Toolbox release is fixed by publishing a newer one, or by reinstalling with
  `install.sh`; there is no local rollback of Toolbox.
