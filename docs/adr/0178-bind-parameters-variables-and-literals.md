# ADR-0178: Bind parameters, script variables, and values written as literals where a driver cannot bind

- **Status:** Proposed
- **Date:** 2026-10-27
- **Supersedes:** the "Parameters" row of ADR-0166, which is not part of this repository,
  for drivers without bind parameters (`.parameter set` for SQLite)
- **Relates to:** [ADR-0170](0170-generated-sql-in-each-drivers-dialect.md) (literals in each
  dialect), [ADR-0172](0172-production-safety-and-agents-on-databases.md) (confirmations)

## Context

Consoles take `:name` placeholders everywhere, `$1` on PostgreSQL and `?` elsewhere, and
script variables (`@set name = value`, `${name}`, `@include file.sql`). The superseded row promised
real bind parameters everywhere, with values never spliced into the text and `.parameter set` for
SQLite.
PostgreSQL and MySQL/MariaDB bind for real. The SQLite driver talks to the `sqlite3` program, whose
`.parameter set NAME VALUE` evaluates VALUE as an SQL expression, so a value must be quoted as a
literal there anyway; ClickHouse's HTTP interface takes typed `{name:Type}` parameters that do not
map to untyped placeholders. Values also must not leak: the query history keeps no values of bind
parameters unless asked, and secret-looking names (`password`, `token`, `api_key`…) are
never kept in files.

## Decision

1. **Drivers that bind, bind.** PostgreSQL gets `$1`…, MySQL and MariaDB `?`, each with its value;
   the value never enters the text.
2. **Drivers that cannot bind get quoted literals.** SQLite and ClickHouse get each value as the
   dialect's literal (`SqlDialect::literal`, ADR-0170), and the run says so in the output
   (`parametersInlined`, values of secret-looking names masked). Placeholders are found by the
   driver's own lexing: never inside strings, comments, quoted names or dollar quotes, and for
   ClickHouse with backslash escapes and backtick names, so a quote in a value can never end a
   string early.
3. **What is kept is what was written.** Each statement carries the text it was written with
   (`QueryStatement::kept`): placeholders as written, and `${name}` of a secret-looking name masked.
   The query history, the statement summaries of a query's state and the confirmations of ADR-0172
   show that text; only the server gets the values. The history keeps values only with the setting
   `parameterValues` (off by default), secret-looking ones masked.
4. **Variables are the IDE's own script commands.** `@set name = value` and `@include file.sql`
   are client commands of every SQL dialect's splitter; `${name}` is text and is expanded
   everywhere, strings included, before placeholders are bound.
5. **Values are remembered per console** in `<data folder>/databases/variables.json`
   (`{ <console path>: { variables: { name: value }, parameters: { name: { type, value } } } }`),
   never values of secret-looking names;
   those stay in memory for the run of the IDE. A secret left empty in the Bind Parameters dialog
   is not sent, so the remembered value is used.

## Consequences

- On SQLite and ClickHouse the server sees literals; the person is told, and nothing kept on disk
  or answered about a statement holds a parameter value unless `parameterValues` is on.
- A result read again (Refresh, Fetch Next, filters) sends the values it was read with; its kept
  statement text is the sent one, values included, for drivers that cannot bind.
- `${…}` is not escaped: a literal `${x}` in a script asks for a variable `x`.

## Alternatives considered

- **`.parameter set` for SQLite.** Its value is an SQL expression, so it would be quoted as a
  literal all the same, with one more command per value and per statement.
- **Refusing placeholders on drivers that cannot bind.** Scripts that use placeholders would stop
  working on SQLite.
- **Keeping the sent text in the history and masking values in it afterwards.** A value is not
  always recognisable in the text; keeping the written text is exact.
