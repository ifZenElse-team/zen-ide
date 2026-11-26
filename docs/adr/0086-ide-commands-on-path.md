# ADR-0086: The IDE commands on PATH

- **Status:** Proposed
- **Date:** 2026-10-21
- **Amends:** [ADR-0043](0043-toolbox-settings-and-shell-launchers.md) (launcher names, the default)

## Context

The owner (2026-10-21): after installing, `gogarden` (and the other IDEs, and zen-cli) must run by
name, not as `/Applications/GoGarden.app/Contents/MacOS/gogarden`. ADR-0043 gave Zen Toolbox shell
launchers, but named them by the image part of the product name (`garden`, `ronin`), kept them off
by default, and nothing put their folder on `PATH`. AI agents are told to run
`gogarden mcp --headless .` and `gogarden --headless .`; zen-cli had no place in a release at all.

Editing a user's shell profile is invasive; not doing so leaves most macOS users, whose
`~/.local/bin` is not on `PATH`, without working commands.

## Decision

1. **Names.** A launcher is named by the product id, the IDE executable's own name: `gogarden`,
   `rustronin`, ... (docs/naming.md). Zen Toolbox deletes its own launchers of the old names
   (`garden`, `ronin`, ...; only marked files, ADR-0043 decision 8).
2. **zen-cli ships with every IDE** as `Contents/MacOS/zen` (`bin/zen` outside a bundle);
   `cargo xtask release` builds and copies it. Zen Toolbox writes a `zen` launcher for the zen-cli
   of the first installed IDE that ships one, and removes it with the last.
3. **On by default.** `toolbox.shellScripts.enabled` defaults to `true`; the location stays
   `~/.local/bin`. Launchers follow installs, updates, uninstalls and a change of the location, as
   before. Toolbox keeps launchers only in a location it was given (the setting or the user's
   default), never a fallback, so a Toolbox without one (tests, the demo) writes nothing.
4. **PATH, with consent.** Toolbox's Settings page (and `toolbox.shellScripts.status`) asks the
   user's login shell whether the folder is on the `PATH` of a new terminal window. When it is not,
   the page shows the exact line, `export PATH="$HOME/.local/bin:$PATH"`, and one button, Add to
   PATH, which appends a comment and that line to `~/.zprofile` (created if missing) and to
   `~/.bash_profile` if it exists, once. Nothing else ever edits a shell file: only that click, the
   IDE dialog's checkbox, or an agent's explicit `toolbox.shellScripts.addToPath` /
   `tools.commandLineLauncher.create` with `addToPath: true`.
5. **In each IDE,** Tools → Create Command-line Launcher… (`view.commandLineLauncher.open`) writes
   that IDE's launcher alone (name and folder editable, `~/.local/bin` by default) through
   `tools.commandLineLauncher.create`; the `PATH` line is a checkbox, unticked until the user ticks
   it. The same marker rule applies: a file Zen did not write is never replaced.
6. **Commands shown to people** (Settings → Tools → MCP Server's `claude mcp add …`) use the short
   name when a terminal finds this IDE's launcher (Zen's script, in a folder on `PATH`), the full
   executable path otherwise.
7. The scripts and the profile line are `zen_update::scripts`, shared by Toolbox and the IDEs.

## Consequences

- `gogarden .`, `gogarden --headless .`, `gogarden mcp --headless .` and `zen …` work from any
  folder once the folder is on `PATH`; a launcher passes every argument on, and the IDE now opens
  `gogarden main.go:12` at that line.
- Existing users get the scripts at their next Toolbox start; their shell files stay untouched
  until they click.
- `on PATH` depends on the login shell answering within a few seconds; fish users are told the
  folder is not on `PATH` and get a line for zsh and bash.
- A second `gogarden .` while GoGarden runs starts a second process; handing the folder to the
  running IDE is separate work.

## Alternatives considered

- **Symbolic links into `/usr/local/bin`:** needs administrator rights and cannot carry a marker.
- **Editing profiles without asking:** what some installers do; it surprises people and breaks
  dotfile managers.
- **Only showing the line:** leaves the commands broken for most users who do not edit shell files.
- **Keeping the short names (`garden`):** agents and people type the executable's name; two names
  for one program are one too many.
