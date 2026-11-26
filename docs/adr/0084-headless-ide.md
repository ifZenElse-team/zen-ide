# ADR-0084: A headless IDE for AI agents: the same app, sessions and MCP server without a window

- **Status:** Proposed
- **Date:** 2026-10-21
- **Relates to:** [ADR-0033](0033-ai-access-to-every-api.md) (every capability reachable by AI),
  [ADR-0007](0007-everything-is-a-command.md) (everything is a command),
  [ADR-0042](0042-zen-cli-output-and-exit-codes.md) (zen-cli's output and exit codes)

## Context

Agents use the IDE over MCP for what a shell lacks: the language server's problems after an edit,
usages, rename, run and test results, Git state. Today that needs a window. A covered window or a
sleeping display stops frames, and part of a project's work advanced only inside a drawn frame
(`IdeView::show`): the language servers' sync (so gopls problems never arrived), the Git status
poll, new runs, usages and debug sessions being followed. Every check also meant starting and
stopping the GUI. An agent wants to keep the IDE open for a whole session and ask it instead of
grep, with no window, no GPU and nothing drawn.

## Decision

1. **The same application, without winit.** `<ide> --headless [<project>...]` builds the same
   `IdeApp` (`zen_ui::ide::run_headless`): the same registry, modules, plug-ins, language servers,
   sessions and commands, and the same MCP server (`zen-automation`). It never creates a window,
   an event loop or a GPU context: an `egui::Context` with no integration only carries
   `request_repaint` as the wake-up signal, and only `IdeApp::logic` runs, through
   `egui::Context::run_logic`, which runs no egui pass (nothing is laid out or drawn).
2. **A project's work is a step of the session, not of a frame.** `IdeView::step` does what the
   frame did for the session: workbench messages into the notifications, terminals followed, the
   Git status and operation, the language servers' sync and their list, usages, runs, the
   Services tool window's requests and debug sessions. A drawn window runs it inside its frame as
   before; `IdeApp::logic` runs it for every window that was not drawn since the last logic: a
   covered or sleeping window, a project window whose own viewport is not drawn, and every window
   of a headless IDE. Timers read a monotonic clock when headless (egui's input time stands still
   without passes).
3. **The headless loop** (`ide/headless.rs`) runs `logic` whenever something asks for a repaint
   (an agent's call, a language server's message, the file watcher, a run's output, a timer
   asked for with `request_repaint_after`) and at least every half second, on its own clock.
4. **Window-only tools answer an error at once instead of waiting for a frame.** `run_action`,
   `press_keys`, `get_window_state`, `get_tree_selection`, `select_in_tree` and `dispatch_command`
   with a window action (`view.*`) answer "not available in a headless IDE", naming the commands
   to use instead. `view.ide.exit` is the exception: it exits. Everything else (`list_commands`,
   `dispatch_command` with every command, `list_windows`, `open_project`, `close_project`,
   `get_document_text`, the terminal, plug-in, theme, keymap and settings tools, `find_action`)
   works as in a windowed IDE.
5. **Lifecycle.**
   - MCP is always on: the port is `--mcp-port <port>`, else `ZEN_AUTOMATION=<port>`, else a free
     one. When it listens, the IDE prints one JSON line, `{"url": ..., "pid": ..., "projects": [...]}`,
     on standard output and records the instance in `<data>/headless/<pid>.json`.
   - `exit_ide` and `view.ide.exit` stop it at once (no Confirm Exit: there is nobody to ask):
     runs, debug sessions and background tasks are stopped as on exit, documents are saved, the
     language servers are shut down when the sessions drop, and the instance file is removed.
   - `--exit-with-parent` ends it the same way when the process that started it ends (Unix: the
     parent process id changes).
   - A second headless start on a project a live headless instance already has is refused with
     exit code 3 and a message naming that instance's URL and process id; `zen-cli ide start`
     reuses it instead. A file whose instance no longer answers is removed.
   - The data folder is the IDE's own (`ZEN_DATA_DIR` as usual). A headless IDE leaves what the
     windows remember alone: the recent projects, which projects open at the next start,
     `workspace.json`, `automation.json` (the windowed IDE's URL for `<ide> mcp`) and the system
     clipboard. It does not update or restart itself, and offers no first-start choice of areas.
6. **zen-cli manages instances** (`zen-cli ide start|list|stop|call`) through the instance files
   and the MCP server; it is a thin client of the same protocol, not a second way into the IDE.
7. **Every IDE gets it**: the flag is parsed by `zen_ui::ide::HeadlessOptions::from_args`, which
   each application calls before it opens a window.

## Consequences

- A covered or sleeping window now advances on its own too: problems, Git status and runs are
  current when an agent asks, drawn frame or not.
- The window build is unchanged when drawn: the step runs inside the frame at the same place as
  the code it replaces.
- What only a window shows (popups, dialogs, tool window layout, keys) is not reachable headless;
  agents use the commands behind them. A new tool must say whether it needs a window.
- A headless IDE and a windowed one may share a data folder; the settings, plug-ins, themes and
  language server choices are shared files, written by whichever changes them.

## Alternatives considered

- **Draw into an offscreen egui pass without a GPU.** Every window tool would work, but the IDE
  would still lay out the whole UI many times a second for nobody, and a frame would remain the
  thing work waits for.
- **A separate headless binary.** It would duplicate the product's start-up (modules, plug-ins,
  language servers) and drift from the windowed IDE.
- **Only zen-cli** (a fresh session per call): no language servers or indexes survive between
  calls, which is what agents need the IDE for.
