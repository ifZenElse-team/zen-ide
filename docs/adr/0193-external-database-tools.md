# ADR-0193: External database tools: found outside the project, passwords in private files, agents kept to the IDE's own choices

- **Status:** Proposed
- **Date:** 2026-11-01
- **Relates to:** [ADR-0143](0143-hidden-files-out-of-agents-searches-and-short-tool-schemas.md) (files hidden
  from agents), [ADR-0172](0172-production-safety-and-agents-on-databases.md) (production safety),
  [ADR-0179](0179-connections-closed-to-agents-hide-their-metadata.md) (closed connections)

## Context

Backup and restore run `pg_dump`, `pg_restore`, `psql`, `mysqldump` and `mysql`; the Migrations tab
runs the project's migration tool (Flyway, Liquibase, golang-migrate, goose, Atlas, sqlx, Diesel,
Alembic, Django, Prisma, Rails, Knex). These are programs the IDE did not build, started with a
connection's stored password, sometimes by an agent. Three things can go wrong: the IDE runs a
program it should not, the password leaks to other programs of the user, and an agent uses the tool
to get past the IDE's own rules.

## Decision

1. **Where tools come from.** Only absolute folders of `PATH` that are outside the open project,
   Homebrew's and Postgres.app's folders, the system's folders, and the database's own Docker
   container (`docker exec`). A relative `PATH` entry, a folder inside the project (a `bin` that
   direnv adds, `node_modules/.bin`, a virtualenv) and anything else the project brings is never run
   on its own; nothing is ever downloaded (no `npx` without `--no`, no installer). A driver
   setting that names a program (`program`, `javaHome`) is passed to the driver only from the
   person's own connections, never from a project's connection file, and must be absolute.
2. **The password never goes into a tool's environment or arguments**, where other programs of the
   user can read it (`ps`, `/proc/<pid>/environ`). It goes into a new file only the user can read
   (mode 0600, created new, never a link) in the tool's own format, which the tool is pointed at:
   a `.pgpass` line named by `PGPASSFILE` (libpq, lib/pq, pgx, sqlx), a `[client]` option file
   given first as `--defaults-extra-file=` (MySQL and MariaDB clients), a Flyway `-configFiles=`
   or a Liquibase `--defaults-file=` properties file. For `docker exec` it is copied into the
   container and removed there. The file is removed when the run ends however it ends. A tool that
   can take a password only in a URL or an environment variable (Prisma, Rails, Knex, Django,
   Alembic, and the MySQL forms of golang-migrate, goose, Atlas, sqlx and Diesel) gets none from the
   IDE: it is refused when the connection needs one, and the person runs it with their own
   configuration.
3. **Inherited password variables are removed** (`PGPASSWORD`, `MYSQL_PWD`, …) from the tool's
   environment, so the IDE's own environment cannot hand one over either.
4. **Names cannot become options or connection strings.** A PostgreSQL database goes to libpq as
   `--dbname=dbname='…'`; a MySQL database or table name, or a Django app, that starts with `-` is
   refused.
5. **Changing tools ask, and only on open, writable connections.** A restore and a migration run
   change the database: they are refused on a read-only connection, refused while the connection is
   closed (the migration preview lists what will run from the applied state, which needs it open),
   and always ask first, as ADR-0172 has statements ask. The checks are made where the run starts,
   whatever started it (the command, the Run tool window, a saved configuration).
6. **Agents get only what the person would.** An agent cannot pass its own tool arguments
   (`extraArgs`), never changes the settings remembered for the person, never reads or writes a file
   hidden from agents (ADR-0143) through a backup, restore or migration, and follows the connection
   type's agent access (ADR-0172).

## Consequences

- No tool the project ships runs by itself; a person who wants one adds its folder to `PATH` outside
  the project or runs it in the terminal.
- Some migration tools cannot get the stored password from the IDE; they run on connections without
  a password, or with their own configuration in the terminal.
- A password with a line break cannot be written into a `.pgpass` line: such a run is refused with
  a message.

## Alternatives considered

- **Environment variables** (`PGPASSWORD`, `DATABASE_URL`): the tools' common way, but readable by
  every program of the user and inherited by children the tool starts.
- **Running project-local tools with a one-time approval.** Worth adding later as an explicit,
  remembered choice; until then they are not run.
