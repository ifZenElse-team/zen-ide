# MongoDB plug-in changelog

Every release of the MongoDB plug-in (`plugins/mongodb`), newest first; the format is in
[../README.md](../README.md).

## 0.1.0 - 2026-11-02

### Added

- The first release: **MongoDB** in the Database workspace, through the `zen-db-mongodb` driver
  program of our own: `mongodb://` and `mongodb+srv://` addresses, SCRAM and X.509 logins, TLS,
  replica sets, databases, collections, views and indexes in the navigator, a console for `find`,
  `aggregate`, counts, inserts, updates and deletes, documents as rows or JSON, editing documents in
  the grid, transactions in manual commit, and Cancel. Nothing else to install.
- The driver program is downloaded with the plug-in for your computer, from the release server, and
  checked by its SHA-256 from the signed plug-in catalog before it is used; nothing runs when it is
  installed.
