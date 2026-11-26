# ADR-0172: Production safety: read-only connections, confirmations before statements run, and what agents may run on each connection type

- **Status:** Proposed
- **Date:** 2026-10-27
- **Relates to:** [ADR-0033](0033-ai-access-to-every-api.md), [ADR-0143](0143-hidden-files-out-of-agents-searches-and-short-tool-schemas.md),
  [ADR-0165](0165-database-connections-types-and-secrets.md) (connection types)

## Context

ADR-0165 gives every connection a type with safety defaults; Production confirms SQL
execution and data changes. Nothing yet stopped a statement: a read-only connection relied on the
server alone, a type that "confirms SQL execution" confirmed nothing, `DELETE FROM orders` without a
`WHERE` ran at once on every type, and an agent over MCP (ADR-0033) could run anything a person can,
on Production too. What is wanted: a question before data-changing statements on types that say so,
with Yes, Skip, Yes to All and Cancel for scripts, and a warning about `DELETE` and `UPDATE` without
`WHERE`.

Agents are the new risk: ADR-0033 makes every action reachable by them, but a confirmation that an
agent can give itself is no confirmation, and ADR-0143 already keeps agents from what a person
hides from them. Production data needs the same: an agent must never get past a person's
confirmation silently.

## Decision

1. **Statements are judged by our own tokens per dialect** (`zen_db::safety`, over the line lexers
   of ADR-0164 decision 6): what a statement does (`read`, `session`, `transaction`, `data`,
   `structure`, `other`) and its risks: `DELETE` or `UPDATE` without a `WHERE` (or with one that is
   always true: `true`, `1 = 1`, `x = x`, `… OR 1 = 1`), in common table expressions too, `DROP` and
   `ALTER … DROP`, `TRUNCATE`, and `FLUSHALL`/`FLUSHDB` for key-value databases. Comments and
   strings never count; a read calling a function with side effects (`nextval`,
   `pg_terminate_backend`, …) is a change; anything not known to be safe is `other`. Each driver
   answers through `Driver::assess`, so a language of its own (Redis) judges its own commands.
2. **Read-only is enforced twice, for every driver**: the session is read-only on the server where
   the database has one (PostgreSQL `default_transaction_read_only`, MySQL and MariaDB
   `TRANSACTION READ ONLY`, SQLite `query_only`, ClickHouse `readonly`, Redis refusing writing
   commands), and the engine refuses, before sending, every statement that is not a read, a session
   setting or transaction control, including those that would make the session writable again.
   A driver that judges writes by asking the server (Redis, through `COMMAND INFO`;
   `Driver::guards_read_only`) is left to judge a person's commands itself, so a read the IDE does
   not know is not refused.
   A connection is read-only by its own setting (the toolbar's lock, `db.connection.readOnly`,
   applied at once to its open sessions) or by its type's new `readOnly`. Saving edits is refused
   the same way.
3. **Confirmations happen before anything runs.** Where the type confirms SQL execution, every
   statement that changes something asks; on every type, a risk asks. The run is refused with code
   `confirmExecute`, the statements that ask, their risks in words and a `token` bound to the
   connection and the exact statements; running again with `confirmed: token` (and `skip`, the
   indexes to leave out) runs them, and nothing else does. The window asks one statement at a
   time with Yes, Skip, Yes to All and Cancel; a risky statement runs only after its risk is ticked
   and Yes to All is not offered past it; Enter and ⌘⏎ never answer, and the window's shortcuts
   wait while it asks.
4. **Agents follow the connection type** (`agents`, overridable per connection): `full` (Development
   and Test) lets an agent do what a person does, confirming with the refusal's token where the
   type asks; `readOnly` (Production's default; a type stored before this setting existed gets it
   when it confirms SQL execution) runs an agent's statements in a read-only session and
   refuses every change, saving edits, committing and changing keys with code `agentReadOnly` and a
   `handOff` (`db.console.new` with the SQL) for a person to review and run; `none` refuses
   everything with code `agentNotAllowed`. An agent may make a connection or a type safer, never
   less safe (read-only off, a looser type or override). The rules apply to every way an agent runs
   SQL: `db.query.run`, a console's commands, filters and object reads.
5. **Everything is in the query history**: refused statements (outcome `refused`, with the code:
   `readOnly`, `agentReadOnly`, `agentNotAllowed`, `skipped`), statements that ran after a
   confirmation (`confirmed`), and agents' statements (`agent`, as before).
6. **Production is visible**: a console of a type that confirms SQL execution is framed in the
   type's theme colour, its toolbar shows the type and the read-only lock, and its tabs carry the
   colour (ADR-0165, decision 2).

## Consequences

- An agent on Production reads freely and hands every change to a person; on Development it works
  as before, confirming risky statements explicitly.
- A refusal costs one round trip; the token makes a confirmation exact: an edit of the console in
  between asks again.
- Our classification is conservative: an unknown statement asks or is refused, and the server's
  read-only session catches what tokens cannot (a function that writes, called in a `SELECT`).
- `connection-types.json` gains `readOnly` and `agents`; files written before keep working (absent
  `agents` is derived from `confirmExecute`).

## Alternatives considered

- **Asking inside the engine while a script runs**, statement by statement. A
  question in the middle of a run holds a session and, on Production, an open transaction while a
  person reads; asking before the run decides everything first. Rejected.
- **Letting agents confirm with `confirmed: true` everywhere**, as `db.result.save` did. On
  Production that is a confirmation nobody gave. Rejected for types where agents only read.
- **A server-side parser for every dialect.** Classification must work before connecting and for
  every driver; our tokens plus the server's read-only session cover it. Rejected.
