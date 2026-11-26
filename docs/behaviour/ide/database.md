# Database workspace: layout, menus and defaults

> **Last updated:** 2026-11-26 · The Database workspace has a model of its own; everything around
> it is the IDE's · **Details:** [database-workspace.md](../../database-workspace.md)

This page specifies, in our own words, how the Database workspace is laid out, which menus and
defaults it has and which keys it uses. What is built and how is in
[database-workspace.md](../../database-workspace.md); the clean-room rules are ADR-0002.

## Layout

- The **Database** button on the right stripe expands the workspace over the editor area. One
  toolbar row on top: New Console, the connection and schema selectors, the read-only lock, the
  commit-mode control, Commit, Rollback and the pending-transaction counter, Find in Database,
  Compare, the query history; a narrow bar moves the less used actions into a ⋯ menu.
- Left: the **Database Navigator**, with its own toolbar and a filter box under it.
- Centre: the workspace's tabs. The **Overview** is always first and not closable: with no
  connections it lists the first actions (New Connection…, New Console); later it lists the
  connections with their state.
- Under a console: the results panel (a header line, a tab per result set, Output and the Execution
  Log).
- AI chat panels, dashboards and accounts are out of scope.

## New connection

- One dialog: drivers on the left with a filter, the driver's fields beside, on the pages Main,
  SSH/SSL, Advanced and General; Test Connection, Cancel and OK.
- The pages hold the driver's settings (host, port, database, authentication or a URL), driver
  properties, SSH tunnels and TLS, and the general page (name, folder, connection type, read-only).
- Drivers are compiled in or shipped by plug-ins; nothing is downloaded when connecting.

## Connection types

New connections are Development.

| Setting | Development | Test | Production |
|---|---|---|---|
| Auto-commit | on | on | off |
| Confirm SQL execution | off | off | on |
| Confirm data changes | off | on | on |
| Smart commit | off | off | off |
| End idle transactions after | 1800 s | 900 s | 600 s |
| Close idle connections after | 14400 s | 7200 s | 3600 s |
| Statement timeout | none | none | 300 s |
| Colour | none | the theme's green | the theme's red |

A type has an id, name, colour and the settings above; people add their own. Production also lets
agents only read.

## Database Navigator

- A tree of connections and their objects, varying by database: databases or catalogues, schemas,
  then folders per object kind with the objects inside.
- Toolbar: New Connection ▾, Connect, Reconnect, Disconnect, Refresh, Collapse All, Link with Editor
  and Options (view preset, Show System Objects, Show Host, Show Descriptions, Show Statistics,
  Show Connected Only, Hide).
- Context menu of a connection: SQL Editor ▸ Open Console, Edit Connection…, Duplicate Connection,
  Connect, Reconnect, Disconnect, Copy Name, Rename…, Delete…, Refresh. Of an object: View Data,
  Open Object, SQL Editor ▸ Open Console, Create ▸, Generate SQL ▸ (DDL, SELECT, INSERT), Tools ▸,
  Copy Name, Copy Qualified Name, Rename…, Delete…, Refresh.
- Every database of a PostgreSQL server is listed.

## SQL editor and execution

- Run the statement at the caret or the selection, run the script, run with each statement in its
  own tab, explain the plan. A script is split on the delimiter (`;` by default); a blank line ends a
  statement Always, Never or Smart. A script stops at the first error and rolls back, or goes on.
- Results open in tabs under the editor, named after the main table (with `(+)` for joins) or
  "Result N"; tabs can be pinned, renamed and closed. Statistics are a header line, not a tab.
- Parameters: `:name` named and `?` anonymous, asked for in the Bind Parameters dialog;
  `@set name = value` defines variables, used as `${name}` or `:name`.
- A statement timeout per connection type (0 for none).

## Data editor

- Above the grid: the filter box (a WHERE condition) with its history and saved filters.
- The views Grid, Text and Record are buttons in the bottom bar; the value panel (with Calc) is
  beside the grid and open by default.
- Bottom bar: Refresh, Save, Cancel, first, previous, next and last row, Fetch Next, Fetch All, the
  fetch size and Calculate Total.
- Edits, new rows and deletions are kept until Save; Cancel drops them. Without a unique key the
  grid does not edit; a read-only connection edits nothing.
- The object editor of a table has the pages Data, Properties, DDL and Diagram; Export Data… in the
  grid's context menu opens the export dialog.

## Shortcuts

The workspace's keys are in [ADR-0186](../../adr/0186-database-keys-and-toolbar-in-the-database-workspace.md); the table is in the guide's
[Running SQL](../../guide/28-database.md#running-sql), and the full list in
[database-workspace.md](../../database-workspace.md).

## Non-relational databases

Redis, Valkey, ClickHouse, MongoDB and others follow each database's own documentation.
