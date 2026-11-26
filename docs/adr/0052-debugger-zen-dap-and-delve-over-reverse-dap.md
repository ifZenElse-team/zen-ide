# ADR-0052: Debugger: zen-dap and Delve over reverse DAP

- **Status:** Proposed
- **Date:** 2026-10-18

## Context

GoGarden must debug Go code: Debug (⌃D) next to Run, line
breakpoints in the gutter, stepping, the Debug tool window (⌘5) with frames, threads (goroutines),
variables and watches, and Evaluate Expression. The Go debugger is Delve, which speaks the Debug
Adapter Protocol (DAP), a public specification; other languages (Rust through an LLDB adapter) speak
it too, so the platform needs one DAP client and an extension point, as it has for runs (ADR-0048).

The forces:

- **No network, no port race.** `dlv dap --listen` makes Delve listen and the IDE connect: the IDE
  must pick a free port for another process, which races, and Delve then accepts any local client
  until one connects. Delve ≥ 1.20 also has `dlv dap --client-addr=<host:port>`: the IDE listens
  first, on a port the system chose, and Delve connects to it. Nothing listens that the IDE does
  not own, and nothing leaves the machine (AGENTS.md rule 10).
- **What Delve builds.** In its `debug` and `test` modes Delve runs `go build` itself, with its own
  flags and output folder, and its errors arrive as one DAP message. Building first with our own
  command lines shows the build in the console as a run does, keeps the binary in the run's scratch
  folder, and leaves Delve one job: `exec`.
- **Layering and clean-room.** The client is a service (L2) and must compile for WebAssembly
  (ADR-0014); it cannot use `zen-lsp`, which is in the same layer. No external DAP crate is taken:
  the types are written from the public specification, the subset we use (ADR-0002).
- **Everything is a command** (ADR-0007, ADR-0033): agents debug with the same commands the UI
  dispatches, and the five parts of the work (Go plans, the workbench service, the toolchain, the
  editor gutter, the tool window) are built at once against one frozen command table.

## Decision

1. **`zen-dap` (L2, `crates/platform/dap`)** is the DAP client; its only dependencies are `serde`,
   `serde_json` and `schemars`, and it starts no process.
   - `protocol`: our serde types of the messages (`Message` = `Request` | `Response` | `Event`,
     told apart by `type`) and of the subset: `initialize` (`InitializeArguments::zen`: client id
     `zen`, lines and columns from 1, `pathFormat` `path`, variable types), `Capabilities` (the
     fields we read), `launch`/`attach` (free JSON), `setBreakpoints`, `setExceptionBreakpoints`,
     `configurationDone`, `threads`, `stackTrace`, `scopes`, `variables`, `evaluate`, `continue`,
     `next`, `stepIn`, `stepOut`, `pause`, `disconnect`, `terminate`, and the events `initialized`,
     `stopped`, `continued`, `output`, `terminated`, `exited`, `breakpoint` and `thread`
     (`DebugEvent`; any other event, such as `module`, is `DebugEvent::Other`).
   - `connection`: `Content-Length` framing (the same 40 lines as `zen_lsp::connection`, duplicated
     because both are L2), our sequence numbers, answers matched by `request_seq` into
     `Pending<T>` (`wait(timeout)`, `try_answer`), events in order on a channel with a waker, every
     waiting request answered `Closed` when the stream ends, and the adapter's reverse requests
     (`runInTerminal`, `startDebugging`, any other) answered `success: false`, "not supported".
     It runs over any `Read` + `Write` pair (pipes in tests) or a `TcpStream` (`Connection::tcp`).
   - `client`: `Client` with typed requests and `handshake(launch, breakpoints, timeout)`:
     `initialize`, then `launch` without waiting, then the `initialized` event (before or after the
     launch response), `setBreakpoints` for every file, `setExceptionBreakpoints` with no filters,
     `configurationDone`, and the launch response. Events that arrive meanwhile are kept for the
     caller. Errors are `DapError`: `Io`, `Closed`, `Timeout`, `Adapter { message }`.
   - `reverse`: `ReverseListener::bind()` listens on `127.0.0.1:0`; `accept(timeout)` takes the
     first connection from a loopback address, and the listener closes with it.
   - `fake` (feature): `FakeAdapter`, a scripted adapter over pipes or a TCP connection to a
     `ReverseListener`, for headless tests of every layer above.
2. **The extension point is `zen_run::RunProvider::debug`** (default `None`): for a configuration
   it gives a `DebugPlan { build, adapter, launch }`. `build` are command lines run first as a run
   runs them, each only when the one before exited with 0; `adapter` is an `AdapterLine { line,
   address_flag }`, started with one more argument, `<address_flag>=<host:port>` of the IDE's
   listener; `launch` is the `launch` request's arguments. `RunContext::debugger` is the adapter
   found by the toolchain (empty when there is none); an adapter line with an empty program means
   the debugger is missing. The path has one source: the UI gives the toolchain's `dlv` to
   `Runs::set_debugger` when a window opens and whenever the toolchain changes, and both runs and
   debug sessions read it from there. `RunProvider::explain_debug_error(message)` (default `None`)
   turns an error the adapter gave into advice for people; a failed start appends it to the error. For Go, `go.build` builds with `go build -gcflags=all=-N -l -o
   <scratch>/<name>` and `go.test` with `go test -c -gcflags=all=-N -l -o <scratch>/<name>.test`;
   the adapter is `dlv dap --client-addr`; `launch` is `{ "request": "launch", "mode": "exec",
   "program", "args", "cwd", "env" }` (tests get `-test.run`/`-test.v` arguments).
3. **The session lives in `zen-workbench`** (the session state of debug sessions, beside `Runs`):
   `debug.session.start` saves the documents, runs the build lines in a run (its console is the
   session's console), binds a `ReverseListener`, starts the adapter, accepts its connection,
   performs the handshake with the breakpoints of the project, and follows events. Stopping sends
   `disconnect { terminateDebuggee: true }` and kills the adapter if it does not exit in 2 s.
   Breakpoints are the user's, not the project's: they are saved in the window's workspace file
   (`workspace.json`) with the selection and the temporary configurations, never in `.zen/`.
4. **The commands** (every id three segments; lines and columns count from 0 in every parameter
   and result, as in all our commands; the service converts to and from the adapter's lines from
   1; `sessionId` and `threadId` default to the current session and its stopped thread):

   | Command | Parameters | Result |
   |---|---|---|
   | `debug.session.start` | `{ configurationId? \| context? \| configuration? }` | `{ sessionId, runId }` |
   | `debug.session.stop` | `{ sessionId? }` | nothing |
   | `debug.session.list` | `{}` | `{ sessions }` |
   | `debug.session.get` | `{ sessionId?, waitMs? }` | `{ state, reason?, threadId?, frame?, threads, stops }` |
   | `debug.execution.resume` | `{ sessionId?, threadId? }` | nothing |
   | `debug.execution.pause` | `{ sessionId?, threadId? }` | nothing |
   | `debug.execution.stepOver` | `{ sessionId?, threadId? }` | nothing |
   | `debug.execution.stepInto` | `{ sessionId?, threadId? }` | nothing |
   | `debug.execution.stepOut` | `{ sessionId?, threadId? }` | nothing |
   | `debug.execution.runToCursor` | `{ path?, line? }` | nothing |
   | `debug.frames.list` | `{ sessionId?, threadId?, start?, count? }` | `{ frames }` |
   | `debug.frame.select` | `{ frameId }` | nothing |
   | `debug.variables.list` | `{ sessionId?, frameId? }` or `{ reference, start?, count? }` | `{ variables }` |
   | `debug.expression.evaluate` | `{ expression, frameId?, context? }` | `{ result, type?, reference }` |
   | `debug.breakpoint.toggle` | `{ path?, line? }` | `{ breakpoint }` |
   | `debug.breakpoint.set` | `{ path, line, enabled?, condition? }` | `{ breakpoint }` |
   | `debug.breakpoint.remove` | `{ path, line }` | nothing |
   | `debug.breakpoint.list` | `{ path? }` | `{ breakpoints }` |
   | `debug.watch.add` | `{ expression }` | nothing |
   | `debug.watch.remove` | `{ expression }` | nothing |
   | `debug.watch.list` | `{}` | `{ watches }` |

   - `debug.session.start` takes the parameters of `run.process.start` and means the same: none is
     the selected configuration, `context` what is at a place (the caret by default).
   - `debug.session.list` gives `[{ sessionId, runId, name, state }]`, oldest first, running and
     ended; `state` is named as in `debug.session.get`.
   - `debug.session.get`: `state` is `starting`, `running`, `stopped` or `ended`; `reason` is the
     stop's (`breakpoint`, `step`, `pause`, `exception`, `entry`) or why it ended (`exited`,
     `stopped`, `failed`, `buildFailed`); `frame` is `{ path, line, name }` of the selected frame;
     `threads` are `[{ id, name }]`; `stops` counts the stops, so that a stop at the same place
     again is told apart. With `waitMs` it waits up to that long while the session starts or runs.
   - Breakpoints follow the edits of open documents; the UI calls `zen_workbench::debug::sync`
     every frame so that moved and changed breakpoints reach a running session at once (the
     breakpoint commands sync too).
   - `debug.frames.list` gives `[{ id, name, path?, line, column }]`; `debug.frame.select` makes a
     frame the one variables and evaluation use.
   - `debug.variables.list` with a frame gives its scopes, with a `reference` the children:
     `[{ name, value, type?, reference, indexed?, named? }]`; a `reference` of 0 has no children.
   - `debug.breakpoint.toggle` acts at the caret of the active editor by default. A breakpoint is
     `{ path, line, enabled, verified?, message?, condition? }`; `verified` and `message` come from
     the adapter while a session runs. Breakpoints change in a running session at once.
   - `debug.watch.list` gives `[{ expression, value?, type?, reference?, error? }]`, evaluated in
     the selected frame of a stopped session (`context: "watch"`).
   - A start whose adapter line has no program fails with the error code `debuggerMissing`
     (`CommandError::code`, serialised only when set), so that the UI and agents can offer to
     install Delve.
5. **Window and keys** (the macOS keymap): ⌘5 is the window action
   `view.toolWindow.debug`; ⌃D is `debug.session.start` with `{}` and ⌃⇧D with `{ "context": {} }`;
   F8 step over, F7 step into, ⇧F8 step out, ⌥F9 run to cursor, ⌥⌘R resume, ⌘F8 toggle
   breakpoint, ⌘F2 stops the session as it stops a run.
6. **Delve** is found by `zen-toolchain` (`dlv` beside the SDK's `go` in `GOBIN`, `GOPATH/bin` or
   `PATH`), and is used only at version 1.20 or later (`dlv version`). It is installed (`go install
   github.com/go-delve/delve/cmd/dlv@latest`) only when the user or an agent asks for it
   explicitly: that downloads code (AGENTS.md rule 10). Nothing is installed on its own.

**Out of scope for now:** attaching to a running process, remote and container debugging, core
dumps, function, exception, data and log breakpoints in the UI (the protocol types have log
messages and hit conditions), setting variables, disassembly and memory views, reverse execution,
hot code replacement, inline values in the editor, and debugging from the browser build (the
client compiles for WebAssembly; sockets and processes do not work there).

## Consequences

- Any DAP adapter that can connect back (or a later listening mode) plugs into the same commands;
  a language module only builds and describes a `DebugPlan`.
- The IDE opens a socket for the first time: on the loopback interface, only while a session
  starts, for one connection from a loopback address. Delve gets the address on its command line.
- The build is ours, so its errors appear in the console like a run's, and the binary goes away
  with the run's scratch folder.
- The framing is duplicated in `zen-lsp` and `zen-dap`; a fix in one is made in the other.
- `CommandError` gains an optional `code`; errors without one serialise as before.

## Alternatives considered

- **`dlv dap --listen` and the IDE connecting:** the port is chosen for another process (a race),
  and a listening socket accepts other local clients until the IDE connects.
- **Delve's `debug` and `test` modes:** Delve builds with its own flags in its own folder, and a
  build error is one message instead of a console with links.
- **The `dap` crate or another DAP library:** an external dependency for a small, stable subset,
  and its types follow another client's needs; ours are written from the specification.
- **Sharing the framing through a new L1 crate:** a crate for 40 lines; layering would allow it,
  but the duplication is smaller than the crate.
- **Breakpoints in `.zen/`:** they would be committed and shared with everyone; we keep
  them in the user's workspace.
