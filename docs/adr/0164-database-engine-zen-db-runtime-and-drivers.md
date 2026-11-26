# ADR-0164: The database engine: `zen-db` in L2, one runtime of its own, drivers by data model, compiled in

- **Status:** Proposed
- **Date:** 2026-10-26
- **Relates to:** [ADR-0002](0002-open-source-clean-room-pure-rust.md), [ADR-0004](0004-monorepo-and-layering.md),
  [ADR-0014](0014-refined-layer-model.md), [ADR-0031](0031-plugins.md), [ADR-0040](0040-https-pure-rust-tls.md),
  [ADR-0050](0050-products-are-plugin-bundles.md), [ADR-0055](0055-language-servers.md),
  [ADR-0061](0061-http-client.md), [ADR-0071](0071-built-in-modules.md),
  [ADR-0077](0077-incremental-highlighting-with-line-lexers.md), [ADR-0144](0144-code-is-parsed-in-pure-rust.md),
  [ADR-0163](0163-workspaces-full-tools-in-the-editor-area.md), [ADR-0165](0165-database-connections-types-and-secrets.md)

## Context

The Database workspace (ADR-0163) needs an engine: connections, drivers, reading a
database's structure, running statements and keeping their results. It must fit the platform's
rules: pure Rust in the default build (ADR-0002), strict layers (ADR-0004, ADR-0014), every action a
command (ADR-0007), nothing on the UI thread, and no network by default (AGENTS.md rule 10).

The owner's decisions shape it: database work must be fast and reliable with current techniques
(2026-10-25); and (2026-10-26) the drivers must let people
connect **any** database, non-relational ones and Redis included, not only SQL databases.

A database tool that downloads JDBC drivers on first use and asks for a "local client" for some
tasks fails when either is missing ("client home is not specified"). JDBC and ODBC need a JVM or C
libraries, which we do not ship.

The crates named below were checked on crates.io and in their repositories on 2026-10-26: versions,
licences, features and what each feature turns on.

## Decision

1. **Where it lives.**
   - A new platform crate **`zen-db`** in **L2** (services, beside `zen-http` and `zen-vcs`): the
     connection model, the driver interface and the drivers, introspection, statement splitting,
     the operations and the result store. It is registered in `xtask/src/layers.rs` with this ADR,
     before the crate exists (the registry lists planned crates, ADR-0014). Like `zen-http`, it
     compiles for `wasm32` as a model only: the drivers, the runtime and the processes are behind
     `cfg(not(target_arch = "wasm32"))`.
   - Commands in **`zen_workbench::database`** (`db.*`), state in the session; the views in
     **`zen-ui`** (the workspace host of ADR-0163 draws them).
   - It is platform code in **every IDE** (ADR-0050), **switched on per product** the way built-in
     modules are (ADR-0071: `modules.json`, Settings → Plugins → Built-in, applied at a restart):
     on by default in **SqlScroll** (docs/naming.md), off and offered in the other IDEs (Settings →
     Plugins → Built-in, and the offer of ADR-0165 when a project names a database). When it is off,
     no `db.*` command is registered and the Database button is not shown.
   - **Not a WebAssembly plug-in.** A plug-in today has no sockets, no processes and no UI of its
     own (ADR-0031, ADR-0056); a driver needs all three. SQL highlighting stays in the SQL plug-in
     (`plugins/sql`), which consoles use like any `.sql` file.
2. **One runtime of its own.** All database I/O runs on **one tokio runtime on a dedicated thread**
   (`zen-db`), started at the first connection, never on the UI thread and never inside another
   service's runtime. Blocking work (the HTTP drivers' `ureq` calls, the `sqlite3` program's pipes,
   decoding large pages) goes to that runtime's blocking pool.
   - A command **starts an operation and returns at once** with its id; progress, rows and the end
     arrive as events kept in the session's database state, and a `zen_run::Waker` wakes the window
     to draw them. Agents read the same state through commands (`db.operation.state`, paged).
   - **Every operation has a cancellation token and a deadline.** Cancel stops the local work and
     asks the server to stop too (PostgreSQL's cancel request, MySQL `KILL QUERY` on a second
     connection, ClickHouse `KILL QUERY` by query id, an interrupt to the `sqlite3` program); a
     deadline that passes cancels the same way. Connecting has its own deadline (default 15 s).
   - **Three execution contexts per connection**: one for reading metadata
     (the navigator, completion), one main context for the data editors, and one per console, so a
     long query in one console never blocks the tree or another console.
3. **Drivers by data model, not SQL only.** A driver implements one interface (`zen_db::Driver`)
   and answers what it can do; the navigator, the editors and the result views ask it instead of
   assuming tables and SQL.
   - **Data models:** `relational` (SQL: PostgreSQL, MySQL, SQLite, ClickHouse, SQL Server),
     `keyValue` (Redis, Valkey), `document` (MongoDB), `search` (Elasticsearch, OpenSearch),
     `wideColumn` (Cassandra, ScyllaDB). A driver may be more than one (ClickHouse is relational and
     analytical; Redis with its JSON module stores documents).
   - **Capabilities** (each a flag or a small list): the query language and its dialect (`sql:
     postgres`, `redis`, `mongo`, `cql`, `elasticsearch`), transactions and their modes, schemas and
     catalogues, the node kinds of its tree, explain plans, cancelling on the server, real bind
     parameters, editing results (and what identifies a row), DDL preview, server-side paging,
     sessions and killing them, read-only sessions, and the URI forms it accepts (ADR-0165).
   - **The tree is the driver's:** `children(node)` returns nodes of a shared vocabulary (catalog,
     schema, table, view, column, index, key prefix, key, collection, index of a search engine, …)
     with their own kind for anything else, each with an icon kind, a label and whether it has
     children; the navigator draws whatever comes back.
   - **One value type for every model:** null, bool, integer, decimal (kept as text), float, text,
     bytes, date and time **as the server sent them** (text plus a type name), JSON, arrays,
     maps and documents. A result is a stream of pages of rows of such values with column names
     and types; a document result is rows of documents that the views show as a table, JSON or a
     tree; a key-value result is one value with its type and TTL.
4. **The compiled-in drivers and their order.** Drivers are **compiled into the IDE**: no
   downloads, no driver manager, no "client home". Each passes `deny.toml` as written today
   (licences on its allow list; no `ring`,
   `aws-lc`, OpenSSL or native-tls, so `cargo tree -e normal -i cc` stays empty for the desktop
   build), with TLS through rustls and the process's RustCrypto provider (ADR-0040). In order:
   1. **PostgreSQL**: `tokio-postgres` 0.7.18 (MIT OR Apache-2.0) with
      `tokio-postgres-rustls` 0.14.0 (MIT; no provider unless its `ring` or `aws-lc-rs` features
      are on, so we turn on neither) and `postgres-protocol` 0.6.12 (MIT OR Apache-2.0, pure
      Rust: SCRAM with `hmac`, `sha2`, `md-5`). Rows are read from the open stream page by page; if
      a spike shows the row decoding is the cost, results are decoded on our own path
      over `postgres-protocol`'s messages.
   2. **SQLite**: the **system `sqlite3` program** (part of macOS: 3.54.0 on the team's Macs,
      2026-10-26), one process per execution context, fed over standard input with `.mode quote`
      and `.headers on` (typed literals, blobs as `x'…'`), `.parameter set` for real bind
      parameters, and an interrupt to cancel. It is SQLite itself, with SQLite's own locking, so a file another program is using is
      as safe as with any SQLite client. Found on `PATH` or at a set path like `git`
      (ADR-0047); without it, the connection says what to install.
   3. **MySQL and MariaDB**: `mysql_async` 0.37.1 (MIT OR Apache-2.0) with `default-features =
      false` and the features `minimal-rust` (flate2's Rust backend instead of zlib) and
      `default-rustls-no-provider` (rustls without a provider, flate2's Rust backend, derive,
      TLS 1.2); `prefer_socket(false)`, so a `localhost` connection uses TCP as configured, not a
      Unix socket the server may not have. `mysql_common` 0.37.1 comes in without its default
      zlib.
   4. **Redis and Valkey**: `redis` 1.8.0 (BSD-3-Clause) with `default-features = false` and
      `tokio-rustls-comp` (rustls and tokio-rustls without default features, so no provider),
      `cluster-async`, `sentinel` and `streams`; keys are listed with `SCAN`, never `KEYS`.
   5. **ClickHouse**: its HTTP interface through the HTTP stack of ADR-0040 (`ureq`, rustls,
      RustCrypto), as `zen-http` sends requests (ADR-0061), with typed result formats
      (`JSONCompactEachRowWithNamesAndTypes`) read as a stream. The `clickhouse` crate 0.15.3 is
      not used: it maps rows to Rust types known at compile time, while an IDE shows any query.
   6. **Elasticsearch and OpenSearch**: their REST APIs over the same HTTP stack. The official
      crates are not used: `elasticsearch` has only pre-releases (9.1.0-alpha.1) and `opensearch`
      2.4.0 defaults to native-tls through reqwest.
   7. **MongoDB**: `mongodb` 3.9.1 (Apache-2.0) **after** it can use rustls without choosing a
      provider. Today its `rustls-tls` feature turns on `rustls/ring` and its alternatives are
      aws-lc and OpenSSL, so every TLS build brings C. We offer the upstream project a feature that
      leaves the provider to the application, and add the driver when it is released. No fork.
   8. **Cassandra and ScyllaDB**: `scylla` 1.9.0 (MIT OR Apache-2.0), on the same condition: its
      `rustls-023` feature takes `rustls` and `tokio-rustls` with their default features, which
      choose aws-lc.
   9. **SQL Server**: `tiberius` 0.13.0 (MIT/Apache-2.0), on the same condition: its `rustls`
      feature takes `tokio-rustls` with default features (aws-lc), and its default `native-tls`
      is OpenSSL on Linux. Not before an upstream fix.

   **Out of the default build:** Oracle (its clients need Oracle's C libraries), DuckDB (C++), ODBC
   (a C driver manager and C drivers) and JDBC (a JVM). Each can come through the extension path
   below; none is compiled in.
5. **Any other database: external driver processes.** A database we do not compile in is served by
   a **driver program** that the IDE starts and talks to over standard input and output with a
   documented JSON-RPC protocol of ours (`docs/database-drivers.md`, written with the first such
   driver): `initialize` (the driver's data models and capabilities, as in decision 3), `connect`
   (the settings, and the secrets in the message, never in arguments or the environment),
   `children`, `execute` (rows as notifications page by page), `cancel`, `close`. A plug-in
   declares such a driver in `plugin.json` with how to install it, like a language server
   (ADR-0055, ADR-0128): installed only on request, started only when a connection of that driver
   connects, stopped when its last connection closes. This is how a JDBC bridge (on a JDK the IDE
   already finds, ADR-0147), an ODBC bridge, Oracle or DuckDB can come, written in any language,
   without C in the IDE and without a crash in a driver taking the IDE down.
6. **SQL text is ours.** Statements are split by our own **line lexers** per dialect (ADR-0077's
   model: state at each line start, so an edit re-lexes only what changed), which know quotes,
   comments, dollar-quoted bodies, `DELIMITER` and blank lines; they find the statement at the
   caret for ⌘⏎. PostgreSQL's syntax tree, for completion contexts and the SQL preview of
   structure changes, comes from **`squawk-parser` and `squawk-syntax`** 2.70.0 (Apache-2.0 OR MIT,
   pure Rust on `rowan`, error-tolerant), as ADR-0144 allows a pure-Rust parser. **`sqlparser`**
   0.63.0 (Apache-2.0), if used for other dialects, only with `default-features = false` and
   `std`: its default `recursive-protection` brings `recursive` → `stacker` → `psm`, which compile C
   and assembly with `cc`. Other query languages (Redis commands, CQL, MongoDB's JSON commands,
   Elasticsearch requests) get their own small lexers in the same style.
7. **The result store.** A result is kept as pages of rows in memory, read from the **open** stream
   or cursor as the view needs them (PostgreSQL's row stream, MySQL's streamed result,
   ClickHouse's streamed body, Redis `SCAN` cursors, MongoDB cursors, CQL paging state); the next
   page never re-runs the query. Each result has a memory budget (default 256 MiB for all results
   of a window): past it, the result stops reading and says so, and **Fetch All** goes on only when
   asked. Exports read the stream to a file without keeping it.
8. **Gate rules for the crate.** When `zen-db` lands, the same change adds `ring`, `aws-lc-rs`,
   `aws-lc-sys`, `openssl-sys`, `native-tls` and `libsqlite3-sys` to `deny.toml`'s
   `[bans] deny`, so a driver feature that would bring one of them fails the gate, not a review.
   Tests never touch the network: drivers are tested against local servers started by the tests
   where available, and against recorded protocol exchanges otherwise.

## Consequences

- SqlScroll can be a bundle (ADR-0050) the day the engine exists; GoGarden, RustRonin and the rest
  get the same workspace by switching it on.
- MongoDB, Cassandra/ScyllaDB and SQL Server wait for upstream TLS changes. We open those requests
  with the first driver; until then those databases are reachable only through an external
  driver.
- SQLite depends on a program that every Mac has, but Windows does not; the Windows build (made
  only on request, AGENTS.md rule 12) says what to install. If `turso_core` reaches 1.0 with
  SQLite's multi-process locking, it can replace the program (see below).
- The driver interface is wider than SQL from the start, so the navigator, data views and consoles
  are written against capabilities; a relational-only shortcut in a view is a defect.
- One more runtime thread exists while a database is connected; nothing starts before the first
  connection.

## Alternatives considered

- **A WebAssembly plug-in per database.** No sockets, processes or UI in the ABI (ADR-0031); adding
  them is the large host API ADR-0071 decided not to build now. Rejected for the engine.
- **WebAssembly driver plug-ins with sockets provided by the host**, as the extension path. A
  sandbox is attractive, but the host would need sockets, TLS and async I/O in its ABI, results
  would cross an interpreter (`wasmi`) row by row, and existing drivers in Java, Go or Python could
  not be used. External processes reuse what language servers already do and let any language
  write a driver. Kept as a later option, not chosen.
- **`sqlx`** (0.9.0). One API for PostgreSQL, MySQL and SQLite, but its SQLite is `libsqlite3-sys`
  (C), its TLS choices pick a provider, and its compile-time query model is not what an IDE needs.
- **`turso_core`** (0.8.3, MIT) for SQLite. Pure Rust only without its default features (`simd`
  brings `simsimd`, C) and with `pure-rust-crypto` (its `aegis` compiles C otherwise, except on
  macOS); before 1.0 its project says compatibility is "not at 100% yet" and multi-process WAL
  coordination is experimental. An IDE opens files other programs are writing, so not now.
- **`rusqlite` with bundled SQLite as an opt-in native feature.** C in the build, even if opt-in,
  for something the system program already does. Rejected; the external driver path can host it
  out of process if ever needed.
- **Driver downloads on first use.** Network by default, version drift, and connections that fail
  until a driver or a client is installed. Rejected.
- **One runtime per connection, or the UI thread polling futures.** Many threads for many
  connections, or work on the UI thread. Rejected for one runtime with cancellation per operation.
- **JDBC through an embedded JVM, ODBC through a C driver manager.** Out of the pure-Rust build
  (ADR-0002); possible only as external drivers.
