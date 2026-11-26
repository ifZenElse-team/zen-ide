# ADR-0111: A second start hands its folders and files to the running IDE

- **Status:** Proposed
- **Date:** 2026-10-22
- **Amends:** [ADR-0086](0086-ide-commands-on-path.md) (its last consequence),
  [ADR-0084](0084-headless-ide.md) (a headless option)
- **Relates to:** [ADR-0079](0079-zen-macos-unsafe-exception.md) (no `unsafe` for peer
  credentials on macOS)

## Context

ADR-0086 put `gogarden` on `PATH` and made `gogarden main.go:12` open the file at that line, but
left one thing for later: with GoGarden already running, the command started a second GoGarden,
with its own windows, language servers and indexes, on the same data folder. The command
should open the file or folder in the IDE that runs (in the window of the project that holds it, or
a new window for a new project) and return at once.

Two IDEs on one data folder also write the same settings, recent projects and workspace files.

## Decision

1. **One IDE per data folder takes the second starts.** A windowed IDE listens on a Unix domain
   socket in its data folder, `<data>/ipc/open.sock` (`zen_workbench::handoff`). A start that would
   open windows (`<ide> [<folder|file[:line[:column]]>...]`, from a terminal, a shell script of
   Zen Toolbox or anything else) first connects to it; when the running IDE takes the paths, the
   start ends with 0 at once and nothing else in it starts. No paths bring the IDE to the front.
   Nobody listening (no IDE, a stale socket of a crashed one, an IDE that is exiting, no answer
   within 5 s) means the start opens its windows as before. Every IDE gets this from the platform
   (`zen_ui::ide::read_command_line`, `IdeApp::new`); the apps change nothing.
2. **What the running IDE does** is what the same command line does at a start: a folder opens as
   a project (a new tab, or a window with Open in New Window) or comes to the front if it is open;
   a file opens in the editor of the open project it lies in, else its folder opens as a project
   with it; `:line[:column]` puts the caret there. The window that shows the last path comes to the
   front. A file inside an open project now opens in that project at any start, not as a project
   of its own folder.
3. **Only the same user.** The socket's folder is created `0700` (a folder opened to others is
   closed again; one that another user owns, or a symbolic link, is refused) and the socket is
   `0600`; a client never talks through a folder that others may enter or that the user running it
   does not own. When the socket's path is longer than a socket may be (100 bytes kept), the folder
   is `zen-<first 16 hex digits of the SHA-256 of the data folder's path>` in the system's
   temporary folder, checked the same way. On Linux both ends also check the other's user through
   the socket (`SO_PEERCRED`, through `rustix`). macOS has `getpeereid` only through `unsafe`, which
   ADR-0079 allows for the quit hook alone, so there the folder's and the socket's modes are the
   check: connecting to a Unix socket needs search permission on its folder.
4. **Not a remote control.** The channel carries one JSON line, `{"open": [<absolute path>...]}`,
   at most 64 KiB and 256 paths, and answers `{"accepted": true|false}`; anything else is refused.
   It runs no command and reaches no network; it opens what the same user could open by running
   the command themselves. A request has 5 s in all to arrive, and each client is served on a
   thread of its own (at most 64 at once), so a client that connects and never writes, or writes a
   byte at a time, holds up neither the other starts nor the IDE; the IDE's frame only takes the
   paths from a queue.
5. **Keyed to the data folder.** The socket lives in, or is named by, the data folder, so a start
   with another `ZEN_DATA_DIR` (an agent's, a test's) never reaches the user's IDE, and the
   user's start never reaches theirs.
6. **Headless IDEs do not take second starts** unless started with `--accept-open`: the user's
   headless IDEs belong to agents (`<ide> mcp --headless`, ADR-0085) and share the user's data
   folder, and a file sent there would open where nobody sees it. With the option a headless IDE
   takes them as a window does, which is how scripts and the tests check the whole path without a
   window.
7. **`ZEN_HANDOFF_ONLY=1`** makes a start that no running IDE takes end with 1 and a message
   instead of opening a window: for scripts and tests that must never open one. A snapshot start
   (`ZEN_UI_SCREENSHOT`) never hands over: it must draw its own window.
8. Windows (and the WebAssembly build) has no such socket here: there every start still opens
   windows of its own.
9. **Agents and tests isolate themselves.** A windowed start with the user's data folder reaches the
   user's running IDE and brings it to the front, so every test, script and agent that starts an IDE
   with folders or files sets its own `ZEN_DATA_DIR` (and `ZEN_HANDOFF_ONLY=1` when it must never
   open a window); `AGENTS.md` and `docs/ai-workflow.md` say so.

## Consequences

- `gogarden main.go:12`, `gogarden .` and `rustronin src/main.rs:2:5` open in the running IDE and
  return in a few milliseconds (measured 0.05 s on a debug build).
- Two windowed starts at the same moment can still both open: the first to bind the socket takes
  later starts; the other runs without it. A process killed with a signal leaves its socket, which
  the next IDE replaces after checking that nobody answers on it.
- The data folder gets an `ipc` folder while an IDE runs.

## Alternatives considered

- **A TCP port with a token in a `0600` file** (as `automation.json`): works on Windows too, but
  opens a port every local user can reach and needs a secret; the socket's permissions do the same
  with nothing to leak.
- **macOS Launch Services (`open -b`)**: hands nothing to an IDE started from a terminal or a
  development build, cannot say `:line`, and is macOS only.
- **The MCP server**: off by default and a much larger surface; opening a file must not need it.
- **A lock file only**: tells a second start that an IDE runs, but cannot hand it anything.
