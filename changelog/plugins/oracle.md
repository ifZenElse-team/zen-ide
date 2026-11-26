# Oracle plug-in changelog

Every release of the Oracle plug-in (`plugins/oracle`), newest first; the format is in
[../README.md](../README.md).

## 0.1.0 - 2026-11-02

### Added

- The first release: **Oracle Database** in the Database workspace, through the `zen-db-oracle`
  driver program, which drives **SQLcl**: install SQLcl and Java 17 or newer yourself (the
  connection finds SQLcl on `PATH`, in `SQLCL_HOME`, `ORACLE_HOME/sqlcl` or Homebrew, or where you
  point it). Service names, SIDs, TNS aliases, connect strings, TLS and wallets; schemas with their
  tables, views, packages, procedures, functions and sequences in the navigator; typed results,
  errors at their place, manual commit, Cancel, editing table rows and export.
- The driver program is downloaded with the plug-in for your computer, from the release server, and
  checked by its SHA-256 from the signed plug-in catalog before it is used; nothing runs when it is
  installed.
