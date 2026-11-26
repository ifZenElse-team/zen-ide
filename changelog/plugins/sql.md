# SQL plug-in changelog

Every release of the SQL plug-in (`plugins/sql`), newest first; the format is in
[../README.md](../README.md).

## 0.2.0 - 2026-10-28

### Added

- Folding of statements, brackets, `BEGIN … END` and `CASE … END` blocks, comments, function
  bodies and `-- region` … `-- endregion`.
- The Structure: every statement; tables, views, functions and other objects a script creates by
  their name, a table's columns, a `WITH` query's parts.
- Completion of keywords, data types and built-in functions, in the case you type, what usually
  follows first.
- Syntax problems: strings, quoted names and comments left open, brackets that do not match,
  `CASE` without `END`, a comma before a clause.
- In a console or a file bound to a connection, all of it in that database's dialect (PostgreSQL,
  MySQL, MariaDB, SQLite, ClickHouse); without one, only problems every common dialect agrees on.

## 0.1.0 - 2026-10-16

### Added

- The first release: highlighting for SQL (PostgreSQL, MySQL, SQLite and standard SQL):
  keywords, types, functions, strings, numbers, comments and parameters.
