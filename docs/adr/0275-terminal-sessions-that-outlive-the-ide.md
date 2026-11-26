# ADR-0275: Terminal sessions that outlive the IDE, and agent sessions that come back after a reboot

- **Status:** Accepted (the owner, 2026-11-11)
- **Date:** 2026-11-11
- **Exception to:** ADR-0012, which is not part of this repository (there, terminals end with the
  IDE)
- **Relates to:** [ADR-0050](0050-products-are-plugin-bundles.md) (capabilities are plug-ins for every
  IDE), [ADR-0084](0084-headless-ide.md) and [ADR-0085](0085-headless-ide-per-agent-session.md) (the
  headless IDE and its MCP bridge), [ADR-0111](0111-hand-files-to-the-running-ide.md) (a socket in the
  data folder), [ADR-0033](0033-ai-access-to-every-api.md) (every capability is a command)

## Context

People run AI agents (Claude Code, Codex, Qwen Code and others) in the IDE's terminal for hours. Today
every terminal is a PTY owned by the IDE process (`zen_terminal::Session`, alacritty's event loop in
a thread of the IDE): updating the IDE through Zen Toolbox, a crash or quitting ends the shell and
the agent in it, and the conversation has to be found and resumed by hand. The owner asked for
sessions that survive an IDE restart and come back as far as possible after a reboot, **for any
agent**, not only one.

Known approaches:

- **tmux** and **zellij** run a server process that owns the PTYs; clients attach over a Unix socket
  and get the current screen, then the live stream. The server outlives every client and ends with
  its last session.
- **A mux server with a versioned protocol** does the same over a Unix socket, and checks a
  protocol version when a client connects, refusing a mismatch.
- **A separate *pty host* process** of the application keeps the terminals: a window reload
  reattaches to the same processes and replays their buffer. After the whole application restarts
  the processes are gone; "revive" puts back the old output, the folder and the environment, with a
  new shell.
- **Restoring the terminal tabs only** (name, folder) after a restart, with new shells; the
  processes end with the IDE.

## Decision

1. **A PTY host process owns the terminals.** A small program, `zen-pty-host` (an app crate, the
   same for every IDE), runs detached from the IDE: its own session and process group (`setsid`),
   standard input and output closed. It owns the PTYs, the shells and everything started in them,
   and keeps each session's parsed terminal state with `alacritty_terminal`: the grid, the
   alternate screen, the cursor, the modes, the scrollback up to its limit. One host serves one data
   folder (`ZEN_DATA_DIR`, else the system's), so every IDE and window using that folder share it,
   and a throwaway data folder in a test gets its own.
2. **The socket and its protocol.** The host listens on a Unix socket in the data folder,
   `<data>/terminal/host-<protocol>.sock`, in a folder only the user can open (`0700`, the socket
   `0600`); when that path is longer than the system allows (104 bytes on macOS) the socket is in a
   per-user folder under the temporary directory, named by a hash of the data folder, and
   `<data>/terminal/host-<protocol>.json` says where. Messages are length-prefixed frames: control
   messages in JSON (open, close, resize, input, list, attach, detach, a session's end) and output as
   bytes with the session id and a running sequence number. The first exchange is `hello`: each side
   gives the protocol version it speaks and the oldest it still understands, and they use the newest
   both understand. Changes within a version only add optional fields; anything else is a new
   version.
3. **The binary lives outside the application.** The IDE does not run the host from its own bundle:
   it copies `zen-pty-host` (signed, as shipped) to `<data>/terminal/bin/<protocol>/<build>/` and
   runs that copy, so replacing or deleting the bundle on an update never touches a running host.
   Copies no host runs any more are removed when a new one is made.
4. **Attaching gives the screen, then the stream.** When an IDE attaches to a session, the host
   sends a snapshot of the parsed state, tagged with the sequence number it covers, then the live
   output after that number: nothing lost, nothing twice. The IDE then sends its pane's size; a
   change reaches the program as `SIGWINCH`, and full-screen programs redraw. Raw byte logs are not
   replayed: they grow without end and cannot rebuild an alternate screen.
5. **Lifetime.** Quitting, crashing or killing the IDE leaves the host and its sessions running; the
   next IDE with the same data folder attaches to them and shows the same tabs (the tab layout keeps
   each pane's host session id). **Closing a tab or a pane still ends its processes**, as now (the
   host hangs up the PTY and signals the process group). The host ends itself when it has no session
   and no IDE has been attached for a while (a minute by default); a session whose program exits
   ends as now. On Windows, until a ConPTY host is written, terminals stay in the IDE process.
6. **Updates never end sessions.** A newer IDE attaches to an older host when they share a protocol
   version. When they do not, the old host keeps its sessions until they end, the IDE says so on
   those tabs (they are shown read-only from the old host when it still offers the oldest protocol
   the IDE reads, else listed as running elsewhere), and new terminals open in a new host on the new
   socket.
7. **The MCP bridge reconnects.** An agent in a terminal talks to the IDE through `<ide> mcp`, a
   stdio bridge (ADR-0033, ADR-0085) that is a child of the agent and so lives in the host too. When
   the IDE goes away, the bridge does not exit: it keeps the agent's stdio open, answers calls made
   meanwhile with an error that says the IDE is restarting, and connects again to the IDE of the same
   data folder when it is back, repeating the handshake itself.
8. **After a reboot, the tabs come back, not the processes.** For each tab the IDE keeps its name,
   its folder, the environment it was given, its last screen and its scrollback (bounded, in the
   data folder, readable only by the user; turned off with restoring). After a reboot, or when the
   host died, the tabs open again with the old output dimmed, a line saying the session ended, and a
   new shell in the same folder. **The command that was running is never started again by itself**:
   the tab offers to run the last command again. Restoring can be turned off in Settings.
9. **Agents resume through adapters, and no agent is built in.** The platform defines an *agent
   resume adapter*: how to recognise the agent in a terminal (its program name and arguments), how
   it reports its conversation's id, and the command that resumes that conversation, with the id as
   a parameter. Every terminal gets `ZEN_TERMINAL_ID` in its environment, so an agent's hook (or a
   wrapper) can report its id to the IDE (`terminal.agent.report` over the bridge, or a file under
   `<data>/terminal/agents/`). Adapters are plug-in contributions (ADR-0050); the user can add one
   in Settings for an agent no plug-in knows. Adapters for Claude Code, Codex and Qwen Code are
   shipped, each checked against that agent's current documentation when it is written. After a
   reboot a tab whose agent has an adapter and a known id starts the resume command in the new
   shell; without one, the tab behaves as in point 8.
10. **Every part is a command** (ADR-0033): listing host sessions, attaching, the restore choices
    and the adapters are commands agents can reach, with `terminal.*` ids.

### What is lost after a reboot, and how the UI shows it

The processes: the shell, the command that was running (a build, a server, an agent's turn in
progress) and anything started in the background. Their output up to the reboot is kept. The tab
shows the old output dimmed under a line "Session ended: the computer restarted" (or "the terminal
host stopped"), and a new prompt; an agent with an adapter continues its conversation, but the turn
it was in the middle of is not redone. Environment changes made inside the shell (`export`,
`cd` after the start) are lost; the folder is the one the shell last reported (OSC 7) when it did.

### Why terminals outlive the IDE (an exception to ADR-0012)

Under ADR-0012, which is not part of this repository, a terminal ends with the IDE. Here we
deliberately do more: agents' sessions are long and costly to lose, updates through Zen Toolbox
are frequent, and nothing a user knows about the terminal becomes wrong (closing a tab still ends
it, the tabs and menus are the same). What the user sees beyond that is only that the processes
are still there.

## Consequences

- A new app crate, `zen-pty-host`, and a client in `zen-terminal` behind the same `Session` API the
  UI and workbench use now, so the view, selection, links and commands do not change.
- The host is a second process to ship, sign, copy and clean up; its protocol becomes a compatibility
  promise kept across releases.
- Processes keep running after the IDE quits, which people may not expect: the guide says so, and a
  tab's context menu and the Terminal settings say how to end them.
- Saved scrollback can contain secrets: it stays in the user-only data folder, is bounded, and is
  not kept when restoring is off.
- Tests use a throwaway `ZEN_DATA_DIR`, so each gets its own host and never touches the owner's.

## Alternatives considered

- **Run tmux (or zellij) under every terminal.** Proven, but an outside dependency that is not on
  every machine, not on Windows, with its own keys and screen model in the way of ours; rejected.
- **Keep the PTYs in the headless IDE (ADR-0084).** It is the whole IDE: an update replaces it, and it
  is far heavier than a PTY owner; rejected.
- **Replay raw output on reattach.** Unbounded, and wrong for full-screen programs; rejected in favour
  of the parsed snapshot.
- **Resume one agent (Claude Code) in the platform.** Simpler, but against ADR-0050 and the owner's
  request that it work for any agent; rejected for adapters.
