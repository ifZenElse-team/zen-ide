# ADR-0085: A headless IDE per agent session: `<ide> mcp --headless`, shared, ended by its last bridge

- **Status:** Proposed
- **Date:** 2026-10-21
- **Extends:** [ADR-0084](0084-headless-ide.md) (the headless IDE); **relates to**
  [ADR-0033](0033-ai-access-to-every-api.md) (every capability reachable by AI)

## Context

ADR-0084 gave every IDE a headless mode, but an agent still had to start it, keep it, and stop it:
`<ide> --headless` or `zen-cli ide start` before the session, `exit_ide` after. Agents such as
Claude Code start MCP servers as programs with the session and end them with it
(`claude mcp add -s project gogarden -- gogarden mcp --headless .`), and several sessions often
work on one project at once. `<ide> mcp` (ADR-0033) is a stdio bridge, but to the windowed IDE,
which it starts: the owner's window was used and touched. Separately, `gogarden --version` opened a
second window: the IDE took every argument for a folder to open.

## Decision

1. **The command line is read before anything starts.** Every IDE's `main` calls
   `zen_ui::ide::read_command_line` first. `--version`/`-V` and `--help`/`-h` print and exit: no
   window, no MCP server, nothing written to the data folder. `mcp [--headless [<project>]]` runs a
   bridge; a bad command line prints its usage and exits with 2. Messages follow the language in
   the IDE's `settings.json` when the data folder has one (read, never written), else the system's.
2. **`<ide> mcp --headless [<project>]`** (the current folder by default) is a stdio bridge to a
   headless IDE for that project, never to a window
   (`zen_automation::bridge::run_headless`). It uses the live headless IDE of the same data folder
   that has the project open (`<data>/headless/<pid>.json`), or starts
   `<ide> --headless --exit-when-unused <project>` in its own process group, with its standard
   input and output closed. Starts are serialised by a lock file, `<data>/headless/start.lock`, so
   that two sessions starting together get one IDE.
3. **Leases.** While it runs, a bridge holds a lease on its IDE: a file in
   `<data>/headless/<pid>.clients/` that it keeps locked (`std::fs::File::lock`). The operating
   system releases the lock when the bridge's process ends, however it ends (closed input, the
   agent killed, a crash), so the count needs no cooperation and no process ids.
4. **The last bridge ends the IDE.** An IDE started with `--exit-when-unused` counts its live leases
   (`zen_automation::instances::clients`, at most twice a second) and exits as `exit_ide` does when
   none is left for one second (sixty seconds when no bridge came at all). It removes its record
   first, so that new bridges start a new IDE instead of joining one that is ending; a bridge whose
   message cannot reach the IDE finds or starts the IDE again and sends the message once more.
   An IDE started otherwise (`<ide> --headless`, `zen-cli ide start`) is used by bridges but never
   ended by them.
5. **The bridge ends with its session:** when its standard input closes, or (Unix) when the
   process that started it ends.

## Consequences

- One line in `.mcp.json` gives every agent session of a project the IDE; nothing has to be kept
  open, and the owner's windowed IDE is never used.
- A short-lived gap between sessions starts a new IDE (the language server and indexes start
  again); the one-second linger only covers a session that reconnects at once.
- Leases are files in the data folder: a `ZEN_DATA_DIR` must be writable for `mcp --headless`.

## Alternatives considered

- **Reference counting over MCP** (attach and detach calls): a killed bridge never detaches, so
  the IDE would need timeouts or heartbeats; a held file lock is released by the system.
- **`--exit-with-parent` for the bridge's IDE**: the first session's end would stop the IDE under
  the others.
- **One IDE per bridge**: simple, but two sessions on one project would run two language servers
  and indexes and could not see each other's open documents.
