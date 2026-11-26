# ADR-0170: The SQL the IDE generates is written in each driver's dialect

- **Status:** Proposed
- **Date:** 2026-10-27
- **Amends:** ADR-0166, which is not part of this repository: the deviation "Parameters"
  (`.parameter set` for SQLite), and decision 2 (filters and staged edits) for MySQL, MariaDB,
  SQLite and ClickHouse

## Context

The WHERE filter, sorting, quick filters and the statements that save staged edits were written for
PostgreSQL only: identifiers in double quotes (a string in MySQL), `::text` casts, `LIKE` relying on
the backslash as escape character, `IS NOT DISTINCT FROM`, `$1` placeholders, `RETURNING`,
`DEFAULT` in `VALUES` and `ctid`. On MySQL, MariaDB, SQLite and ClickHouse they produced wrong SQL,
and editing was refused because only PostgreSQL bound parameters and named the table of each result
column.

## Decision

1. **One dialect per SQL driver** (`zen_db::dialect::SqlDialect`, chosen from the driver's query
   language) writes every generated piece: identifiers (`quote_for`), string and byte literals, the
   cast to text for `LIKE` and for comparing JSON, `LIKE` with `ESCAPE '!'` (so backslash settings
   never matter; ClickHouse, which has no `ESCAPE`, escapes with backslashes), the null-safe
   comparison (`IS NOT DISTINCT FROM`, `<=>`, `IS`), placeholders, and the order of `NULL`s.
2. **`NULL`s sort as in memory** on every server: last going up, first going down (PostgreSQL's
   default; `NULLS LAST`/`NULLS FIRST` on SQLite and ClickHouse; `col IS NULL, col` on MySQL and
   MariaDB, except for a sort by position, which MySQL cannot qualify).
3. **MySQL string literals never depend on `NO_BACKSLASH_ESCAPES`**: text with a backslash or NUL
   is written as `_utf8mb4 X'…'`.
4. **Editing on MySQL and MariaDB** binds values with the server's prepared statements (`?`, bytes
   through `UNHEX(?)`, booleans written as `TRUE`/`FALSE` because `BIT` and `TINYINT` read the text
   `'1'` wrongly). Rows are identified by the server's column metadata (database, original table and
   column name), and the table by a checksum of its schema and name (`dialect::table_id`), as MySQL
   has no table ids. Without `RETURNING`, a saved row is read back with a `SELECT` by key
   (`LAST_INSERT_ID()` for a new auto-increment key). An `UPDATE` counts the rows it **matched**
   (from the server's info line), so writing a value equal to the stored one is not a conflict.
   `FLOAT` cells are not compared in the conflict check, as MySQL's equality on them is unreliable.
5. **Editing on SQLite** writes values as quoted literals instead of `.parameter set`: the shell
   stores a parameter's value by evaluating it as SQL text, so it needs the same quoting and adds a
   dot-command that the driver refuses by design (one checked statement at a time). The literals are
   safe: strings with quotes doubled (SQLite has no backslash escapes), bytes as `X'…'`, numbers as
   numbers. `sqlite3` does not say which table a result column reads, so a plain `SELECT` of one
   table (`*` or plain column names, with only `WHERE`, `ORDER BY`, `LIMIT` and `OFFSET`) is traced
   to it by its text; anything else is read-only. `DEFAULT` is not SQL in SQLite's `VALUES` and
   `SET`: an insert leaves the column out and an update writes the column's default expression.
   Rows come back with `RETURNING` (SQLite 3.35 and later). `ctid` identifies rows only on
   PostgreSQL; SQLite's `rowid` is not used, as `SELECT *` does not read it.
6. **ClickHouse** filters and sorts in its dialect; its results are read-only with the reason
   `noRowIdentity`. Key-value drivers stay read-only with `driver`.

## Consequences

- Filters, quick filters and generated SQL work on every SQL driver; the console's filter box reads
  each dialect's quotes and comments (backticks, `#` comments, backslash escapes, `[names]`).
- MySQL and MariaDB report `bind_parameters`; SQLite does not, and saves through its own quoting.
- Unit tests check each generated statement per dialect; ignored Docker tests run filters, sorts,
  quick filters, edits and conflicts on MySQL 8.4 and MariaDB 11.4, and tests against the real
  `sqlite3` do the same for SQLite.
