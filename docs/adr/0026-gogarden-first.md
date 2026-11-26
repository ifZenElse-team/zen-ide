# ADR-0026: GoGarden is the first IDE released

- **Status:** Accepted
- **Date:** 2026-10-16
- **Amends:** the plan in README, ROADMAP and AGENTS.md that RustRonin is the first IDE

## Context

The platform comes first, and every IDE shares the same window, editor, project and commands
(ROADMAP, Phase 3). RustRonin 0.1 was released as the first application on top of it. The owner
decided to lead with Go instead, and to stop offering RustRonin for installation for now.

## Decision

1. **GoGarden is the first IDE offered through Zen Toolbox.** Its language module, `zen-lang-go`,
   highlights Go; the rest is the shared platform.
2. **RustRonin is withdrawn from the release manifest** (`zen-releases withdraw`). Its code stays in the
   repository and keeps building and passing the gate; it is released again when the owner decides.
3. Withdrawing a release removes it from the manifest only; artifacts stay on the server, and copies
   already installed keep working until the user uninstalls them.

## Consequences

- Language work for the first IDE targets Go (gopls when semantic features arrive).
- `zen-releases withdraw --product <id> [--version <v>]` is part of the release tooling.
