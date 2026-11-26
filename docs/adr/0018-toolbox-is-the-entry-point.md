# ADR-0018: Zen Toolbox is the entry point

- **Status:** Accepted
- **Date:** 2026-10-16

## Context

ADR-0008 describes Zen Toolbox as one way to install and update Zen IDEs, next to standalone
installers and system package managers. The owner decided that Toolbox is more than an installer:
it is the launcher through which people reach the whole family.

## Decision

1. **Zen Toolbox is the primary entry point** to Zen IDE. The website offers Toolbox first; people
   install it once and get every Zen IDE through it. Standalone installers remain available for
   environments where Toolbox cannot run, and keep their self-update (ADR-0008, decision 3).
2. **Toolbox launches the tools.** Clicking an installed tool starts it (`toolbox.tool.launch`);
   a project in the Projects tab opens in the IDE that owns it; shell launchers (`ronin`, ...) are
   created by Toolbox.
3. **Toolbox is the home of everything shared between IDEs:** installation and updates of tools,
   language servers and toolchains, settings shared across IDEs, and the environment selector
   (local now, remote environments later).
4. Toolbox runs in the background with a tray icon and can start at login, so that launching a
   tool is always one click away.

## Consequences

- IDEs report their recent projects in a documented location that Toolbox reads.
- Launching must be fast: Toolbox starts an installed tool without checking the network first.
- Every Toolbox action is a command, so a launcher in the terminal (`zen-cli`) and agents can do
  the same.

## Alternatives considered

- **Toolbox as an optional installer only:** simpler, but splits the experience; shared features
  (toolchains, settings, projects) would have no home.
