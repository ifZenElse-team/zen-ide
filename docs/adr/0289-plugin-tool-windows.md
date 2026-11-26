# ADR-0289: Plug-ins declare tool windows over their declared programs

- **Status:** Accepted (the owner, 2026-11-13)
- **Date:** 2026-11-13
- **Builds on:** [ADR-0031](0031-plugins.md) (WebAssembly plug-ins),
  [ADR-0050](0050-products-are-plugin-bundles.md) (every capability a plug-in),
  [ADR-0286](0286-plugin-run-targets.md) (declared programs, the `plugin` run kind, agent access
  per plug-in)

## Context

A person working with containers wants a tool window that lists the containers, images, volumes
and networks, shows a container's state, and starts, stops, removes and prunes them, opens a
container's log and a shell in it. That belongs in the Docker plug-in (ADR-0050). A plug-in is a
WebAssembly module with no imports (ADR-0031): it cannot run `docker`, and has no UI of its own.
The next tools want the same kind of window over their own program (Kubernetes resources through
`kubectl`, Terraform state, a database's processes): a list of rows per section, and actions on
them.

The request was for the Docker Engine API through the daemon's socket. The platform already runs
programs a plug-in declares (ADR-0286), under the trust and agent rules.

## Decision

1. **Tool windows are declared, in `plugin.json`.** `"toolWindows"`: a list of windows, each with
   an `id`, a `title` per language (`{"en": ..., "ru": ...}`) and `sections`. A section has an
   `id`, a `title`, its `columns` (names, with `columnTitles` per language), a `list` command and
   `actions`. A command is a
   `program`, one of the plug-in's declared programs (`"runs"`, ADR-0286), and its `args`. An action
   has an `id`, a `title`, a `program` and `args`, `rows` (whether it acts on chosen rows) and a
   `kind`:
   - `stream`: a run of kind `plugin` (the Run window: console, Stop), for a log that follows;
   - `terminal`: a Terminal tab running the program, for an interactive shell in a container;
   - `change`: run to its end in the background, once per chosen row; the section is listed again
     when it ends;
   - `destroy`: a change that cannot be undone (remove, prune); the person confirms it every time.

   Settings → Plugins shows the declaration with the declared programs, before anything runs.
2. **A new optional export of plug-in ABI v1, `zen_tool_rows`.** Input: the section's id, a NUL
   byte, then what the list command printed on standard output. Output: UTF-8 JSON, a list of
   rows `{"id": ..., "columns": [...], "state": ...}`: `id` is what actions act on, `columns` are
   the values in the section's column order, `state` (optional) is shown as a dot (`running`,
   `paused`, `exited`...). The host keeps at most 5000 rows, 32 columns and the first line of each
   value, cut at 1000 characters, and drops rows without an id. The plug-in only reads text, as
   for its other exports.
3. **The platform runs everything, through the `docker` CLI, not the daemon's socket.** This
   departs from the request on purpose. The CLI uses the person's Docker context and credentials
   (`docker context`, registry logins, a remote daemon over SSH), which a client of our own would
   have to find and read again; there is no second Docker client to keep in step with the
   daemon's API versions; and the trust rules are the same as for every program a plug-in
   declares (ADR-0286): a declared bare name, looked up as tools are (`PATH`, the usual folders),
   an argument list as its `argv`, never a shell. The plug-in never opens a socket, a file or the
   network.
4. **Row values come only from rows the platform listed.** An argument of a declared action may be
   exactly `${row.id}` or `${row.<column>}`; the whole argument is replaced by that value, as one
   `argv` item, never part of a longer argument (an argument that holds `${row.` anywhere else is
   refused). The values are those of rows the platform itself listed in this session for that
   section. A caller, a person's click or an agent's call, passes row ids; the platform looks them
   up among the listed rows, and an id it did not list is refused. No value is ever taken from the
   caller's text.
5. **Trust.** In a project nobody trusted, or in safe mode, nothing runs: listing and every action
   are refused with the reason, for the person and for agents alike, since listing runs `docker`
   too.
6. **Agents.** Commands: `plugins.toolWindow.list` (the declared windows), `plugins.toolWindow.rows`
   (a section's rows) and `plugins.toolWindow.act` (an action on listed rows).
   - Listing is read-only and open to agents in a trusted project.
   - `stream`, `terminal` and `change` follow the plug-in's agent setting of ADR-0286 (`none` by
     default, `ask` with a `confirmation` token, `full`). A `terminal` action counts as a change: a
     shell in a container gives the agent arbitrary execution inside that container.
   - `destroy` asks the person every time, whatever the setting (unless it is `none`, which refuses
     it): the agent gets `confirmationNeeded` with a token, a notification asks the person, and
     only the person's yes (`plugins.toolWindow.allow`, refused to agents) lets the same call with
     the token run once. A person's own destroy is confirmed in the window's dialog.
7. **Refresh.** The window lists a visible section every 5 seconds, and never while an action on
   that section runs (the rows answer `busy`). After a failed listing (the daemon is not running)
   it waits 30 seconds before it lists by itself again, and shows the error in the window, not in a
   notification each time; the Refresh button lists at once. A failed change or destroy posts one
   notification with what the program printed.

## Consequences

- The Docker window is the Docker plug-in's declaration and a parser of `docker ... --format
  '{{json .}}'` lines; nothing about Docker is in the platform.
- Other tools (`kubectl`, `terraform`, a database's command-line client) get a window with no
  platform code of their own.
- The platform's run machinery and the terminal stay the only places programs start, so trust,
  safe mode and the agent rules hold for every plug-in's window alike.
- Events (`docker events`) are not followed: the window lists again every 5 seconds while visible,
  which costs one short `docker` process per visible section. A later decision can add a `watch`
  command to a section.
- A tool whose program has no line-per-row output cannot use it; its plug-in parses what there is.

## Alternatives

- **The Docker Engine API over the daemon's socket**, as first asked: a second client, with its
  own discovery of contexts, TLS and credentials; rejected for the reasons in decision 3.
- **A Docker tool window in the platform:** against ADR-0050; each next tool would need its own.
- **Plug-ins draw their own UI:** a UI ABI for plug-ins is a much larger decision, and the window
  would not look like the platform's; rejected for now.
- **Agents pass argument values:** would let an agent's text become a program's argument;
  rejected (decision 4).
