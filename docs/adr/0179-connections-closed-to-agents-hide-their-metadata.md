# ADR-0179: A connection closed to agents hides its structure and statements too, not only its data

- **Status:** Accepted
- **Date:** 2026-10-28
- **Supersedes:** the `none` access of item 4 of
  [ADR-0172](0172-production-safety-and-agents-on-databases.md), which refused statements only
- **Relates to:** [ADR-0033](0033-ai-access-to-every-api.md) (every command reachable by agents),
  [ADR-0143](0143-hidden-files-out-of-agents-searches-and-short-tool-schemas.md) (files hidden from
  agents)
- **Decided by:** the owner, 2026-10-28

## Context

ADR-0172 gave every connection type an `agents` access: `full`, `readOnly` or `none`. With `none`
every statement an agent asked for was refused (`agentNotAllowed`), but the rest of the Database
workspace still answered agents about that connection. The navigator, Go to Database Object and
Search Everywhere, object tabs and their DDL, completion in its consoles, schema snapshots and
comparisons, generated code, and the consoles' text told an agent the names of its schemas, tables
and columns, and the statements people ran there. A type that forbids agents everything was taken
to mean "agents see nothing of it". It did not.

## Decision

1. **`none` closes the connection to agents entirely.** An agent sees no rows, no object, column or
   schema name, no structure and no statement of a connection whose type (or override) gives agents
   no access. A person sees everything as before.
2. **One gate for every `db.*` command.** Commands are registered through
   `database::agent_access::register`. It wraps each one in `ClosedToAgents`, except a short list of
   commands about the connection itself (`OPEN_TO_AGENTS`): its settings, secret, connecting,
   status, read-only lock, type, drivers, imports and settings. These answer no metadata. For an
   agent the wrapper does two things:
   - It refuses a call that names a closed connection, with code `agentNotAllowed`. A call names one
     through `connection`, `connections`, a console's or script's `path`, an object, structure or
     plan `tab`, a result `id`, or a comparison's `source` and `target`. When the parameter is left
     out, the console, tab or result in front counts.
   - It takes out of the answer whatever refers to a closed connection. An item of a list goes; any
     other part refuses the whole answer.

   A command added later is closed by default.
3. **What is not a `db.*` command follows too.**
   - The Database workspace's inner tabs of a closed connection are left out of
     `workspace.host.state` for agents, and `workspace.tab.select` refuses them.
   - Saved schema snapshots of a closed connection are left out of `db.compare.snapshots` and
     refused as a comparison side, wherever the file was copied.
   - Files that hold such metadata join the files hidden from agents (ADR-0143) while any connection
     is closed: the consoles of closed connections, their snapshots, the cached structure, the query
     history, the crash journals, saved filters and the navigator's state.
4. **What stays open.** The connection itself stays visible: its name, driver, host, type and
   status, in `db.dataSource.list` and `db.dataSource.get`. That lets an agent see that it exists and
   is closed, so it can hand the work to a person. Counts in a list (`total`) may still count the
   items that were left out.

## Consequences

- Agents can't use the structure of a closed connection to write SQL for a person, and can't open
  consoles on it either (`db.console.new` is refused); the person does that.
- A test runs every `db.*` command as an agent against a closed connection that a person has been
  working on, and checks that no answer or refusal carries its names. Another test checks that
  every command in `OPEN_TO_AGENTS` exists.
- The wrapper serialises answers to JSON before scrubbing them; commands keep their parameter
  schemas, so MCP tools are unchanged.

## Alternatives considered

- **A check in every command.** Rejected: there are more than 200 commands, and a command added
  later would be open by default.
- **Hiding the connection entirely**, `db.dataSource.list` included. Rejected: the agent could no
  longer tell a person why it cannot help, and could add a second connection to the same server.
