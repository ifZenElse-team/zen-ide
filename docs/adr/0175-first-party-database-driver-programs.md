# ADR-0175: First-party database driver programs live in `drivers/`, speak TLS through rustls with RustCrypto themselves, and never bring C

- **Status:** Proposed
- **Date:** 2026-10-27
- **Relates to:** [ADR-0002](0002-open-source-clean-room-pure-rust.md), [ADR-0004](0004-monorepo-and-layering.md),
  [ADR-0014](0014-refined-layer-model.md), [ADR-0040](0040-https-pure-rust-tls.md),
  [ADR-0050](0050-products-are-plugin-bundles.md), [ADR-0164](0164-database-engine-zen-db-runtime-and-drivers.md)
  (decisions 4 and 5), [ADR-0170](0170-generated-sql-in-each-drivers-dialect.md),
  [ADR-0174](0174-external-database-driver-protocol.md)

## Context

ADR-0174 lets a plug-in serve a database through a driver program. The requirement is the first
ones the team writes itself: SQL Server and MongoDB, then DuckDB and Oracle. ADR-0164 kept SQL
Server, MongoDB and Cassandra out of the IDE because their Rust clients choose a TLS provider that
is C (`ring`, `aws-lc`) or OpenSSL. Three questions follow: where such programs live in the
repository and what the layer and licence checks do with them; whether a program may bring the C
the IDE refuses; and what each of the four databases needs.

The crates were checked on crates.io on 2026-10-27:

- `tiberius` 0.13.0 (MIT/Apache-2.0): its `rustls` feature takes `tokio-rustls` 0.26 with default
  features and calls `rustls::crypto::aws_lc_rs` directly when no provider is installed, so it
  brings `aws-lc-rs` → `aws-lc-sys`; `native-tls` is OpenSSL on Linux. **Without a TLS feature** it
  builds in pure Rust and speaks TDS over any stream the caller gives it.
- `mongodb` 3.9.1 (Apache-2.0): it does not build without a TLS feature (`src/runtime.rs`:
  `compile_error!("At least one of the features 'rustls-tls', 'rustls-tls-aws-lc' or 'openssl-tls'
  must be enabled.")`). `rustls-tls` turns on `rustls/ring` and `tokio-rustls/ring` → `ring`
  0.17.14; `rustls-tls-aws-lc` → `aws-lc-rs` 1.18.2 → `aws-lc-sys` 0.46.0; `openssl-tls` →
  `openssl` 0.10.81 → `openssl-sys` 0.9.117. Every build of the official driver brings C.
- `bson` 3.1.0 (MIT): BSON and Extended JSON in pure Rust (`time`, `uuid`, `indexmap`, `rand`); with
  the RustCrypto hashes (`sha1`, `sha2`, `hmac`, `md-5`) and `stringprep` it is all a client of
  MongoDB's wire protocol needs besides TLS.

## Decision

1. **Where they live.** A first-party driver program is a binary crate in **`drivers/<id>`**
   (`zen-db-<id>`), a member of the workspace, registered in `xtask/src/layers.rs` as the kind
   **driver**: it may depend on other driver crates only (the shared **`zen-db-driver-kit`**,
   `drivers/kit`: the protocol loop, sessions, cancel and timeouts, values, TLS), nothing depends
   on it but tests, and it is built for the desktop only (not in the WebAssembly check). Its
   plug-in is an ordinary plug-in in `plugins/<id>` (an ABI v0 module and `plugin.json` with
   `databaseDrivers`), so it installs from the catalog into every IDE (ADR-0050). No product
   bundles a driver program.
2. **The same rules as the IDE.** A driver crate is checked by the gate like any crate: `deny.toml`
   (its licences, and the bans of ADR-0164 decision 8 on `ring`, `aws-lc-rs`, `aws-lc-sys`,
   `openssl-sys`, `native-tls`, `libsqlite3-sys`), Clippy, formatting, translations of nothing (a
   program shows no text of its own; its messages are the server's or protocol errors). A database
   whose only client needs C is not served by a program in this repository.
3. **TLS is the program's own**, rustls with the RustCrypto provider through the kit
   (`zen_db_driver_kit::tls`), with libpq's modes as the IDE sends them (`disable`, `prefer`,
   `require`, `verify-ca`, `verify-full`, a CA file, a client certificate), as the compiled-in
   drivers have them.
4. **SQL Server** (`drivers/sqlserver`, plug-in `plugins/sqlserver`, driver id `sqlserver`, language
   `sql:sqlserver`): `tiberius` without TLS features on a stream of our own. TDS 7.x encryption
   is negotiated in the pre-login exchange and its TLS handshake travels inside PRELOGIN packets;
   tiberius, built without TLS, refuses every encryption level but "not supported", so it is
   configured so and the program sets the encryption option of tiberius' pre-login request to what
   the connection's mode asks (`ON`, or `NOT_SUP` for `disable`), reads the server's answer as it
   passes, then runs the handshake itself in those packets, TLS 1.2 only (SQL Server takes TLS 1.3
   only with TDS 8.0), and encrypts everything after it. TDS 8.0 ("strict", `tls.direct` or the
   `strict` setting) runs TLS before the pre-login with ALPN `tds/8.0`. So Azure SQL and servers
   that force encryption work without `aws-lc`. Without a certificate check (`require`) the
   handshake's signature is still verified, with the key read from the certificate whatever its
   X.509 version (SQL Server's own certificate is version 1, which webpki does not read). After
   TDS's attention signal the server ends the cancelled request's message and acknowledges the
   attention in another; tiberius stops at the first, so the program joins them. Generated SQL gets
   a T-SQL dialect (`SqlDialect::SqlServer`, ADR-0170): identifiers in brackets, `N'…'` strings, `1`/`0` for booleans, `0x…` bytes,
   `@P1` placeholders (bytes through `CONVERT(VARBINARY(MAX), @P1, 2)`), `CAST(… AS
   NVARCHAR(MAX))` for text, `NULL`s ordered with `CASE`, null-safe equality written out, `[` escaped
   in `LIKE`.
5. **MongoDB is a client of our own** (the owner chose it on 2026-10-27, of the two ways this record
   first offered: an upstream feature without a crypto provider, or our own client).
   `drivers/mongodb` (`zen-db-mongodb`, plug-in `plugins/mongodb`, driver id `mongodb`, language
   `mongo`) speaks the wire protocol itself on `bson`: commands as OP_MSG, the `hello` handshake,
   SCRAM-SHA-256 and SCRAM-SHA-1 through `saslStart`/`saslContinue`, the primary of a replica set
   found from `hello`, cursors read with `getMore` and closed with `killCursors`, Cancel as closing
   the connection and `killOp` of its operations from a new one. TLS is the kit's from the first
   byte. `mongodb+srv://` is looked up with a DNS client of its own (SRV and TXT over UDP, TCP when
   cut, to the servers of `/etc/resolv.conf`), about 200 lines, as no pure-Rust resolver comes
   without features we would have to vet. Statements are a subset of the MongoDB shell read by the
   program; the IDE splits them as a command language that spans lines (`Dialect::Script`: `;` or a
   blank line, JavaScript quotes and comments) and judges them by the verbs the plug-in declares;
   the program itself refuses changes in a read-only session. Documents are rows: top-level fields
   become columns in the order they first appear in the first batch, nested documents and arrays
   are relaxed Extended JSON.
6. **DuckDB** (next): DuckDB is C++, so the program drives the **`duckdb` command-line program**
   the user installs, over its standard streams, as the SQLite driver drives `sqlite3` (ADR-0164
   decision 4.2): files and in-memory databases, Parquet and CSV read by DuckDB itself, nothing
   compiled from C++ in our build.
7. **Oracle** (after it): no pure-Rust client speaks Oracle's thin protocol, and the Rust crates
   need Oracle's C client. The plan is a JDBC bridge program on a JDK the IDE already finds
   (ADR-0147) with Oracle's thin JDBC driver downloaded from Maven Central on the user's word,
   speaking protocol v1; it needs its own ADR for the bridge and the driver's licence.

## Consequences

- SQL Server, Azure SQL included, comes as a plug-in with no C anywhere; the IDE gains a T-SQL
  dialect for its generated SQL, which any other `sql:sqlserver` driver also gets.
- The driver programs are built by `cargo build --workspace` and tested by the gate; their
  integration tests against real servers are ignored and run in Docker (docs/development.md).
- A driver plug-in is published only when the program can be: `cargo xtask release plugin` builds,
  signs and uploads driver programs with their plug-in since
  [ADR-0191](0191-driver-programs-published-with-their-plugins.md); `plugins/sqlserver` and
  `plugins/mongodb` have no version of their own yet, and are not released.
- MongoDB comes as a plug-in with no C, at the cost of a client we maintain; the IDE gains a
  statement splitter for command languages that span lines (`Dialect::Script`). Compression, the
  `MONGODB-X509`, `MONGODB-AWS`, LDAP and Kerberos logins, load-balanced and sharded topologies
  beyond `mongos` itself, and transactions are not in it.

## Alternatives considered

- **Driver programs outside the workspace, free to use `ring` or OpenSSL.** They would escape the
  gate and bring C into what users install; rejected for first-party programs. Third-party plug-ins
  remain free (ADR-0174).
- **A fork or `[patch]` of `tiberius` or `mongodb`.** Rejected, as ADR-0164 rejects forks.
- **`hickory-resolver` for `mongodb+srv://`.** A full resolver for two record types, with features
  that bring `ring` to keep switched off; the two lookups are a small module instead.
- **TDS 8.0 only for SQL Server.** Simpler, but SQL Server 2019 and older and servers without a
  certificate for it would only connect without TLS; the pre-login handshake costs one module.
