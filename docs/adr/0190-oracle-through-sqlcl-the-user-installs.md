# ADR-0190: Oracle Database through SQLcl, the command-line program the user installs

- **Status:** Proposed
- **Date:** 2026-10-31
- **Supersedes:** decision 7 of [ADR-0175](0175-first-party-database-driver-programs.md) (Oracle through a
  JDBC bridge)
- **Relates to:** [ADR-0002](0002-open-source-clean-room-pure-rust.md), [ADR-0164](0164-database-engine-zen-db-runtime-and-drivers.md),
  [ADR-0170](0170-generated-sql-in-each-drivers-dialect.md),
  [ADR-0172](0172-production-safety-and-agents-on-databases.md), [ADR-0174](0174-external-database-driver-protocol.md),
  [ADR-0175](0175-first-party-database-driver-programs.md)

## Context

Oracle Database speaks its own network protocol (Oracle Net with TTC on top). No client of it exists
in pure Rust: the Rust crates (`oracle`, `sibyl`) load Oracle's C client (Instant Client, OCI), and
the only thin clients, which need no C, are Oracle's own for Java (JDBC "thin"), Python, Node.js and
.NET. ADR-0175 planned a JDBC bridge; the transport had to be decided, with three
candidates:

1. **SQLcl the user installs**, as the DuckDB driver drives the `duckdb` program: Oracle's free
   command-line program (Java, on the JDBC thin driver), installed from oracle.com or Homebrew, which
   prints results in machine-readable formats (`SET SQLFORMAT json`, `csv`).
2. **A JDBC bridge**: a small Java program of ours speaking protocol v1, loading Oracle's `ojdbc`
   driver downloaded from Maven Central on the user's word.
3. **A clean-room thin client of our own** in Rust, written from observed traffic and public
   documentation.

The owner chose SQLcl on 2026-10-31.

## Decision

1. **The transport is SQLcl, which the user installs.** `drivers/oracle` (`zen-db-oracle`, plug-in
   `plugins/oracle`, driver id `oracle`, language `sql:oracle`) runs one SQLcl process per session
   over its standard streams, never through a shell, and compiles nothing of Oracle's. Neither the
   repository nor any download of ours carries SQLcl, `ojdbc` or a JDK.
2. **How the program is found**, in this order: the connection's **program** setting (the `sql`
   program, or SQLcl's folder or its `bin`); `$SQLCL_HOME/bin`; `$ORACLE_HOME/sqlcl/bin` (a database
   or client home ships SQLcl there); `sql` on `PATH`; where Homebrew puts it. On Windows the names
   are `sql.exe` and `sql.bat`.
3. **Java.** SQLcl runs on Java 17 or newer, which it finds itself (`JAVA_HOME`, then `PATH`). The
   connection's **javaHome** setting, a JDK's folder, is passed to it as `JAVA_HOME`. When SQLcl
   ends before it answers, the driver tells the cases apart from what it wrote and whether a Java
   can be found (`JAVA_HOME`, a `java` on `PATH` other than macOS's stand-in, macOS's
   `/usr/libexec/java_home`).
4. **Clear errors.** A missing or unstartable SQLcl is a connection error with code `sqlclMissing`
   ("SQLcl is missing: … Install SQLcl from oracle.com/sqlcl …"); a missing or too old Java has code
   `javaMissing` ("Java is missing: … SQLcl runs on Java 17 or newer …"), as DuckDB's has
   `duckdbMissing`. Both name the settings that fix them.
5. **The password never reaches the command line or a log.** SQLcl starts with `-S -R 3 -NOLOGINTIME
   /NOLOG`: silent, restricted at level 3 (no host commands, no scripts, no spool files), not logged
   in. The driver then writes `CONNECT user/"password"@identifier` on SQLcl's standard input, which
   no other process can read; every line SQLcl prints is passed through a filter that replaces the
   password before it reaches an error or the driver's log. A password with a double quote or a line
   break, which `CONNECT` cannot carry, is refused.
6. **Connect identifiers** come from the connection's **connectionType**: `serviceName` (Easy
   Connect `//host:port/service`), `sid` (`host:port:SID`), `tns` (an alias, with **tnsAdmin** as
   `TNS_ADMIN`) or `custom` (a connect string written by the user, Easy Connect Plus or a
   descriptor without spaces). TLS modes Require, Verify CA and Verify Full connect over TCPS with a
   descriptor, `SSL_SERVER_DN_MATCH` on only for Verify Full, a **wallet** folder as
   `MY_WALLET_DIRECTORY`; a wallet `.zip` (Autonomous Database) is opened with `SET CLOUDCONFIG`
   first. The certificate is checked by Java against its trust store or the wallet. **role** connects
   `AS SYSDBA` and the other administrative privileges. Every value is checked to stay on its line.
7. **Output is read, not scraped.** After connecting (which runs the user's `login.sql`), the driver
   sets `SQLFORMAT JSON`, feedback on, substitution variables off (`DEFINE OFF`), blank lines kept in
   statements, no `;` terminator, and a known date format. Each statement is sent as SQLcl's buffer
   followed by `/`, then `PROMPT` of a random marker that ends what it printed. A query's rows are
   read from the JSON document (column names and Oracle types, then items) with numbers kept as their
   exact text; `SET ROWLIMIT` caps them at what was asked for plus one. Changes are counted from the
   feedback (`2 rows updated.`); an error report gives the message, the Oracle code (`ORA-00942`,
   `PLS-00201`) and the line and column (`Error at Command Line : 2 Column : 8`, `ORA-06550: line
   2, column 3`, a compiler log's `3/5`), turned into a position in the statement.
8. **Only SQL and PL/SQL reach SQLcl.** A statement is refused before it is sent when its first word
   is not one of SQL or PL/SQL (SQLcl's own commands such as `HOST`, `@`, `SET`, `CONNECT`, `SPOOL`),
   or when one of its lines is one SQLcl would act on (`/` or `.` alone, a line starting with `#`).
   The IDE splits Oracle scripts itself (`zen_db::sql::Dialect::Oracle`): `q'[…]'` quotes, PL/SQL
   blocks and units (`DECLARE`, `BEGIN`, `CREATE [OR REPLACE] PROCEDURE | FUNCTION | PACKAGE |
   TRIGGER | TYPE`) end only at a `/` line, which also ends any other statement.
9. **Cancel** sends SIGINT to SQLcl's process group, which asks SQLcl to cancel the running statement
   (`ORA-01013`); if SQLcl has not finished three seconds later, the driver ends it, and the next
   request starts and connects a new one with the session's state (schema, isolation, manual
   commit). Timeouts work the same way.
10. **Transactions** follow SQLcl's `AUTOCOMMIT`: on by default, off in manual commit mode, where a
    change, a locking query or a PL/SQL block opens a transaction and a definition (which Oracle
    commits) or `COMMIT`/`ROLLBACK` ends it. The engine's own transaction for saving edits (`BEGIN`,
    which Oracle does not have) turns autocommit off until its `COMMIT` or `ROLLBACK`;
    `RELEASE SAVEPOINT` is accepted and does nothing. Isolation is set with `ALTER SESSION SET
    ISOLATION_LEVEL` (READ COMMITTED, SERIALIZABLE).
11. **Structure** comes from the `ALL_*` views: schemas from `ALL_USERS` (Oracle's own as system),
    tables, views, materialised views, packages (a new object kind, `ObjectKind::Package`, folder
    **Packages**), procedures, functions, sequences and types from `ALL_OBJECTS`, with columns, keys,
    foreign keys, checks, indexes, comments, view queries and PL/SQL sources; a schema's fingerprint is
    its objects' count and latest `LAST_DDL_TIME`. A query of one table (`SELECT … FROM [schema.]table
    …`) gets its columns' table from `ALL_OBJECTS`, so its rows are edited as other databases' are.
12. **Generated SQL in Oracle's dialect** (`SqlDialect::Oracle`, ADR-0170): identifiers bare only
    when they are plain upper-case words, else in double quotes; booleans as `1`/`0`; bytes as
    `HEXTORAW('…')`; `TO_CHAR` for text; `NULLS FIRST`/`LAST`; null-safe equality with `DECODE`;
    `FETCH FIRST n ROWS ONLY` instead of `LIMIT`; no `AS` before a subquery's alias; values written
    as literals, as SQLcl binds nothing.

## Consequences

- Oracle needs no C and nothing of Oracle's in our build or downloads; the user installs SQLcl and a
  JDK, both free, once. The IDE's connection fails with a message that says which is missing.
- A session costs a JVM (about 200 MB and a second or two to start), and every statement a round trip
  through SQLcl's console. Results are not paged on the server: a result is read up to the row cap
  asked for, and one larger than 512 MB of output is refused.
- What SQLcl does not show cannot be had: bind parameters (values are inlined as literals), server
  output of `DBMS_OUTPUT`, execution plans, the exact position of an error SQLcl reports without one.
- SQLcl's output formats are its public, documented formats; a future SQLcl that changes them breaks
  parsing, which the driver's tests with a fake SQLcl pin down and the ignored Docker tests
  (`gvenzl/oracle-free` with a real SQLcl) check.
- Cancel depends on SQLcl answering SIGINT; where it does not (or on Windows, which has no SIGINT for
  a process without a console), Cancel ends SQLcl, and the server may run the statement on until it
  notices the lost connection.

## Alternatives considered

- **A JDBC bridge** (ADR-0175's plan). Full control (paging, binds, cancel through
  `Statement.cancel`), but a Java program of ours to build, sign and ship, and Oracle's driver
  downloaded under its licence. Rejected by the owner in favour of a program Oracle ships and the
  user installs, as with DuckDB.
- **A clean-room thin client in Rust.** No Java at all, but Oracle Net and TTC are large, versioned
  protocols without a public specification, with authentication (O5LOGON, O7L_MR) and native network
  encryption to reproduce; months of work and a long tail of server versions. Rejected.
- **Oracle's C client (OCI, Instant Client) through `oracle` or `sibyl`.** Brings C into the build
  (ADR-0002, ADR-0175 decision 2). Rejected.
- **`SET SQLFORMAT csv`.** Simpler to read, but without the columns' types, which the JSON document
  carries.
