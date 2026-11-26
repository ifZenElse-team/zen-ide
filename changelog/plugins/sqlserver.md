# SQL Server plug-in changelog

Every release of the SQL Server plug-in (`plugins/sqlserver`), newest first; the format is in
[../README.md](../README.md).

## 0.1.0 - 2026-11-02

### Added

- The first release: **SQL Server and Azure SQL** in the Database workspace, through the
  `zen-db-sqlserver` driver program of our own: logins with a password or an Entra ID access token,
  TLS (also strict TDS 8.0), schemas and their objects in the navigator, completion, typed results
  read page by page, editable table rows, manual commit, Cancel and export. Nothing else to install.
- The driver program is downloaded with the plug-in for your computer, from the release server, and
  checked by its SHA-256 from the signed plug-in catalog before it is used; nothing runs when it is
  installed.
