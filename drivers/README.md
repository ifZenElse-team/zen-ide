# Database driver programs

Each folder is a program the IDE starts to reach a database it does not compile in, speaking the
external driver protocol v1 ([docs/database-drivers.md](../docs/database-drivers.md),
[ADR-0174](../docs/adr/0174-external-database-driver-protocol.md)); a plug-in in `plugins/<id>`
declares it, and the IDE downloads it when the plug-in is installed
([ADR-0175](../docs/adr/0175-first-party-database-driver-programs.md)).

| Folder | Crate | Database |
|---|---|---|
| [kit](kit) | `zen-db-driver-kit` | none: the protocol loop, sessions, cancel and timeouts, values, TLS through rustls with RustCrypto |
| [sqlserver](sqlserver) | `zen-db-sqlserver` | Microsoft SQL Server and Azure SQL, over `tiberius` without its TLS features |
| [mongodb](mongodb) | `zen-db-mongodb` | MongoDB, a client of our own over the `bson` crate (OP_MSG, SCRAM, cursors, `mongodb+srv`) |
| [duckdb](duckdb) | `zen-db-duckdb` | DuckDB, through the `duckdb` command-line program the user installs |
| [oracle](oracle) | `zen-db-oracle` | Oracle Database, through SQLcl, the command-line program the user installs (ADR-0190) |

Rules (`cargo xtask layers` checks the first two):

- A driver crate depends on no workspace crate but other drivers; nothing depends on it except
  tests. It is built for the desktop only.
- A database whose only client is C or C++ is reached through that database's own program, which
  the user installs (DuckDB, and SQLcl for Oracle), never compiled into the driver.
- It passes the gate as the IDE does: no `ring`, `aws-lc`, OpenSSL or other C (`deny.toml`), TLS
  through `zen_db_driver_kit::tls`.
- Its program writes nothing but protocol messages to standard output, never a password anywhere.
- Its unit tests run in `cargo test`; tests against a real server are `#[ignore]`d and run in Docker
  ([docs/development.md](../docs/development.md), "Database integration tests"). Every program also
  runs the conformance suite, `cargo test -p zen-db --test external_driver` with `ZEN_DB_DRIVER`.
