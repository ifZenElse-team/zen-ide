# DuckDB plug-in changelog

Every release of the DuckDB plug-in (`plugins/duckdb`), newest first; the format is in
[../README.md](../README.md).

## 0.1.1 - 2026-11-04

### Changed

- DuckDB 1.0 or newer is needed; an older `duckdb` is refused when you connect. The program runs
  without its start-up file (`~/.duckdbrc`) and never downloads an extension by itself: `INSTALL`
  one when DuckDB asks for it.
- AI agents run DuckDB's SQL on the database itself only: reading or writing other files, `COPY`,
  `ATTACH`, extensions and other servers are refused for them and handed to you.

### Fixed

- No line of a statement reaches the `duckdb` program as one of its own commands: a dot-command on
  any line (`.shell`, `.system`) and a line with only `/` or `GO` are refused before the statement
  is sent.

## 0.1.0 - 2026-11-02

### Added

- The first release: **DuckDB** in the Database workspace, through the `zen-db-duckdb` driver
  program, which drives the **`duckdb` command-line program**: install DuckDB yourself (for example
  `brew install duckdb`). Database files and in-memory databases, Parquet, CSV and JSON files read
  with SQL, databases, schemas, tables, views and columns in the navigator, typed results,
  transactions, Cancel and export.
- The driver program is downloaded with the plug-in for your computer, from the release server, and
  checked by its SHA-256 from the signed plug-in catalog before it is used; nothing runs when it is
  installed.
