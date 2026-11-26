# ADR-0176: Structure changes run as one previewed script; migration files are up-only SQL; SQLite rebuilds keep foreign keys

- **Status:** Proposed
- **Date:** 2026-10-27
- **Relates to:**
  [ADR-0170](0170-generated-sql-in-each-drivers-dialect.md) (generated SQL in each dialect),
  [ADR-0172](0172-production-safety-and-agents-on-databases.md) (production safety)

## Context

The object editor stages structure changes (columns, constraints, indexes, schemas, views,
sequences, routines) and shows the SQL they make before anything runs. Three things need a
decision because they are a file format or behaviour people rely on: what Execute runs compared
with what the preview shows, the files **Save as Migration** writes, and how a SQLite table that
cannot be altered in place is changed without losing rows that refer to it.

## Decision

1. **The preview is the script.** `db.structure.preview` answers the statements in the order they
   run, with `BEGIN`/`COMMIT` around them when the server changes structure inside a transaction
   (PostgreSQL, SQLite) and the Main context is in auto-commit with no transaction open; MySQL and
   MariaDB commit every structure statement on their own, which a note says. Execute runs exactly
   those statements in the Main context, stopping at the first error. Production safety
   (ADR-0172) applies, but the script runs whole: its confirmation answers `skippable: false`
   and a `skip` is refused (`allOrNothing`), because leaving a statement out would apply part of
   the changes while every change counts as done.
2. **Migration files are plain SQL, up only.** Save as Migration writes a new file
   `<folder>/<yyyymmddhhmmss>_<slug>.sql` of the open project: the time stamp in UTC, the slug from
   the object and the first change (ASCII letters and digits, `_` between words, at most 60
   characters), `folder` relative to the project (`migrations` by default). The file holds the
   preview's notes as `--` comments in the interface language, then the script with each statement
   ending in `;`. No down script is written: what undoes a `DROP` is not known from the change.
   An existing file is never written over (a new name is taken), and a folder that is not inside
   the project — `..`, an absolute path, or a link that leads out of it — is refused
   (`invalidFolder`). The format is meant for migration tools that run SQL files in name order
   (Flyway-style tools, golang-migrate, dbmate and others take such files with their own naming).
3. **A SQLite rebuild keeps the rows that refer to the table.** SQLite cannot change a column's
   type, nullability or default, or add and drop constraints, in place; the table is rebuilt (a new
   table, the rows copied, the old one dropped, the new one renamed, indexes and triggers created
   again; SQLite's own "generalized ALTER TABLE procedure"). Dropping the old table with foreign
   keys on would delete or change the rows of tables that refer to it (`ON DELETE CASCADE`,
   `SET NULL`), so the script starts with `PRAGMA foreign_keys = OFF` before its transaction.
   SQLite ignores that pragma inside a transaction, so a rebuild is refused
   (`rebuildInTransaction`) when the Main context is not in auto-commit or has a transaction open.
   Execute first reads `PRAGMA foreign_keys` and, when they were on, runs
   `PRAGMA foreign_keys = ON` after the script. A script opened in a console or saved as a
   migration ends with that statement as a comment, since the state of the database it will run on
   is not known; a note says so.
4. **What a statement would lose is refused, not guessed.** A SQLite rebuild that would lose a
   `CHECK`, a generated column, `AUTOINCREMENT`, `COLLATE`, `STRICT` or `WITHOUT ROWID`, and a
   MySQL or MariaDB column restated with `CHANGE COLUMN` that has `ON UPDATE`, `INVISIBLE` or is
   generated (only a rename keeps it) are refused with the reason; the person makes that change in a
   console.

## Consequences

- What runs is what was read in the preview; a confirmation can only accept or cancel it.
- Migration files work with any tool that runs SQL files and need no IDE to apply; a team that
  needs down scripts writes them by hand.
- A rebuild never changes rows of other tables, and never leaves foreign keys off in the IDE's
  session; on a database used outside the IDE the person decides whether to run the commented
  `PRAGMA foreign_keys = ON`.
- A rebuild cannot run inside a manual-commit transaction; the person commits or rolls back first.

## Alternatives considered

- **Let the confirmation skip statements, as for scripts in a console.** A skipped `DROP COLUMN`
  would leave the table half changed while the editor cleared every change as applied.
- **Always end a rebuild with `PRAGMA foreign_keys = ON`.** It would switch foreign keys on for
  sessions that had them off on purpose and change what later statements do.
- **Up and down migrations.** A down script for a dropped column or table cannot bring the data
  back; writing one would promise what it cannot do.
- **Timestamps in local time.** Two people in different zones would order the same migrations
  differently.
