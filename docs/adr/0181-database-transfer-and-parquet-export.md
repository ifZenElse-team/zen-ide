# ADR-0181: Database-to-database transfer and Parquet export through arrow-rs

- **Status:** Proposed
- **Date:** 2026-10-28
- **Relates to:** [ADR-0170](0170-generated-sql-in-each-drivers-dialect.md) (SQL in each dialect),
  [ADR-0172](0172-production-safety-and-agents-on-databases.md) (confirmations and agents),
  [ADR-0002](0002-open-source-clean-room-pure-rust.md) (pure Rust)

## Context

Data Transfer is to copy tables from one connection into another (existing tables by column
name, missing ones created) and export to Parquet. The Database workspace already streams exports
and loads CSV files in batches inside savepoints. Two questions are open: how a transfer between
different servers types its columns and values, and how Parquet is written without C code. The
reference Parquet writer for Rust is `parquet` from the Apache Arrow project (arrow-rs); its default
features pull `zstd` (`zstd-sys`, C) and `brotli`, and the `flate2` default backend may use zlib.

## Decision

1. **Transfers reuse the CSV import's loader.** A table is read page by page in a read-only session
   of its own on the source (`Engine::transfer`) while the pages go through a bounded queue into the
   same batched loader as the CSV import: PostgreSQL's `COPY … FROM STDIN`, else multi-row `INSERT`s
   in the target's dialect, savepoints per batch with the row-by-row retry, one transaction a table
   or a commit every N rows. Tables are copied one after another; cancel stops both servers.
2. **Types go through a portable type.** `zen_db::transfer::SqlType` is read from any driver's type
   name and written in the target's dialect when a table is created; between the same dialect (and
   between MySQL and MariaDB) the source's type text is kept. Values are converted for the target
   column's portable type, and a value that cannot be (a word into a number column) is a row error
   with a stable code, not a failed transfer.
3. **The target's safety applies before anything runs.** `CREATE TABLE`, `TRUNCATE` and the
   `INSERT` head pass `safety::guard` on the target, so read-only connections refuse, a Production
   type asks first, and agents follow each connection's agent access; the source's `SELECT` passes
   the source's agent access. A table is created only when the target's snapshot does not have it,
   never with `DROP` or `IF NOT EXISTS`.
4. **Parquet is written by `parquet` 60 with `default-features = false`** and only the features
   `arrow`, `snap`, `lz4` and `flate2-rust_backend`: Snappy (default), LZ4 (`LZ4_RAW`), gzip through
   flate2's Rust backend (`miniz_oxide`, the one the rest of the workspace uses: `zlib-rs` would
   become every crate's backend by feature unification and change the bytes of committed PNGs) and
   no compression, all pure Rust. Zstandard and Brotli are left out until pure-Rust
   encoders are available; `cargo tree -e normal -i cc` stays empty. Columns are typed from the
   driver's type names (`Decimal128` for exact numbers with a known scale, timestamps in
   microseconds, UTC for those with a zone, text for types without a portable form); rows are
   gathered into Arrow arrays and written in row groups, so one row group is held in memory.
5. **Parquet goes only to files.** The format is binary; `db.export.start` refuses it with the
   clipboard (`binary`).

## Consequences

- One loader serves files and databases, so batches, savepoints, errors and history behave alike.
- Types without a portable form (arrays, enums, geometry) are copied as text between dialects.
- The build gains arrow-rs crates (Apache-2.0); `cargo deny` accepts them.
- Parquet files compressed with Zstandard cannot be written yet.
