# ADR-0173: Database profiles: wire-compatible servers as data over the compiled-in drivers

- **Status:** Proposed
- **Date:** 2026-10-27
- **Relates to:** [ADR-0164](0164-database-engine-zen-db-runtime-and-drivers.md) (drivers by data model),
  [ADR-0165](0165-database-connections-types-and-secrets.md) (the connection files),
  [ADR-0170](0170-generated-sql-in-each-drivers-dialect.md)

## Context

The owner asked (2026-10-27) why many popular databases are missing and whether they can be added
in one general way. Many of them speak a protocol a compiled-in driver already has: CockroachDB,
YugabyteDB, Redshift, Materialize, RisingWave, QuestDB and CrateDB speak PostgreSQL's; TiDB,
SingleStore, Vitess and PlanetScale, StarRocks and Doris speak MySQL's; Valkey, KeyDB, Dragonfly
and Garnet speak Redis's. What differs is small but real: default ports, databases and users, URI
schemes, whether TLS is required, the host names of managed services, and a catalogue that lacks
what our introspection reads (`xmin`, `pg_sequence`, `pg_policy`, `pg_total_relation_size`, session
settings such as `lock_timeout`). Databases with a protocol of their own are the external drivers of
ADR-0164, decision 5, not this ADR.

## Decision

1. **A profile is data over a driver** (`zen_db::profile`): id, name, base driver, icon (its
   family's), default port, database and user, URI schemes of its own, managed-service host
   patterns, words of its version text, TLS default, driver settings it turns on (`pooler`), quirks
   and a link to the vendor's documentation. The table is Rust code compiled into the IDE; it is
   **not** a file format plug-ins add to. Facts come only from public vendor documentation, cited
   in `docs/database-workspace.md`.
2. **Quirks** are a closed set the drivers understand: how the structure is read (`native`: the
   driver's own queries; `informationSchema`: the standard views only, tables, views, columns and
   keys, read over the simple protocol with names as literals; `metaFunctions`: QuestDB's
   `tables()` and `table_columns()`), whether row-version fingerprints exist (otherwise every
   wanted schema is read again), whether the session guards are set (lock and idle-transaction
   timeouts, read-only sessions), whether transactions can be held, whether other databases are
   listed, and which navigator folders are hidden.
3. **A connection names its profile** in the new optional field `profile` of `data-sources.json`
   (ADR-0165's format; older files read as before, unknown ids as no profile). `driver` stays the
   base driver, so everything that works by driver keeps working. A profile chosen for a new
   connection fills the port, database, TLS mode and settings not given; a saved connection keeps
   its own when its profile changes.
4. **A URI picks a profile** by its own scheme (`cockroachdb://`, `redshift://`, `valkey://`,
   `valkeys://` with TLS) or by a managed service's host (`*.neon.tech`, `*.supabase.co`,
   `*.redshift.amazonaws.com`, `*.cluster-*.rds.amazonaws.com`, `*.psdb.cloud`), within the family
   of the driver the scheme picks.
5. **Detection refines, never rewrites.** After connecting, the server's version text (PostgreSQL's
   `SELECT version()` with the TimescaleDB and Citus extensions, MySQL's `VERSION()` and
   `@@version_comment`, Redis's `INFO server`) and the host name name a profile. A session without
   a profile then reads its structure with that profile's quirks, the snapshot keeps the profile in
   effect, `db.connection.status` answers `detectedProfile`, Test Connection answers `profile` and
   the dialog offers **Use the … profile**, and a notification offers the same as
   `db.dataSource.update { id, profile }`. The stored settings change only when the person (or an
   agent) asks.
6. `db.driver.list` answers the profiles beside the drivers, filtered by `query` and `family`,
   capped by `limit` with `truncated`, each with its capabilities (the driver's, with the profile's
   schemes and quirks) unless `capabilities: false`. The connection dialog lists drivers and
   profiles grouped by family with search.

## Consequences

- Twenty-five servers are offered with no driver code; adding one is a table row and its tests.
- Servers on the information-schema path show tables, views, columns and keys only: no sequences,
  routines, triggers or sizes until a catalogue of their own is written for one of them.
- Snapshot files go to format version 4 (they carry the profile); older ones are read again.
- Only profiles whose server images are on the team's machines are run in Docker; the others are
  covered by unit tests of their catalogue queries, version texts and defaults.

## Alternatives considered

- **A profile file plug-ins can add to** (JSON or TOML in `plugin.json`). Quirks are code paths in
  the drivers, so a file could only choose among them; the format would need its own ADR and
  versioning for little gain. Kept as a later step if plug-ins ask for it.
- **A driver per product** (`cockroachdb` as its own driver id). Duplicates the driver, its
  capabilities and its settings for every product and breaks everything keyed by driver id.
- **Silently switching a connection's profile when detection says so.** Changes saved settings
  behind the person's back (ADR-0165); rejected for showing it and offering the change.
