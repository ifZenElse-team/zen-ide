# ADR-0016: Zen Toolbox may use its own colour scheme

- **Status:** Accepted; decisions 1 and 3 amended by [ADR-0280](0280-clean-room-process.md)
- **Date:** 2026-10-16
- **Amends:** ADR-0012, which is not part of this repository, decision 2 ("visual metrics ...
  colours"), for Zen Toolbox only

## Context

The UI specification of the IDEs includes the colours of the default themes. For the IDEs,
colours are part of muscle memory: syntax highlighting, selection, diff and error colours are read
at a glance. For Zen Toolbox, a launcher opened for a few seconds at a time, the colour scheme does
not affect how people use it, while a distinct palette helps the product's own identity. The owner
decided that the Toolbox colour scheme does not have to follow that specification.

## Decision

For **Zen Toolbox only**:

1. Layout, components, sizes, spacing, typography, texts, icons' meaning and placement, and
   behaviour follow the UI specification.
2. Colours (backgrounds, gradients, accents, links, badges) may differ. The palette is defined in our
   own theme tokens, in dark and light variants, and must keep contrast at WCAG AA or better.
3. Toolbox colours that differ from the IDEs' are not defects and are not tracked in
   `docs/behaviour.md`.

The IDEs keep the colours of the UI specification.

## Consequences

- The Zen Toolbox theme can carry the Zen IDE visual identity once it exists.
- Screenshot checks for Toolbox focus on geometry and content, not colour.

## Alternatives considered

- **The IDEs' colours for Toolbox too:** consistent with the IDEs, but gives up identity for no
  usability gain.
