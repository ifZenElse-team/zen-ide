# External database drivers: protocol v1

> **Last updated:** 2026-10-31 · **Decisions:** [ADR-0164](adr/0164-database-engine-zen-db-runtime-and-drivers.md)
> (decision 5), [ADR-0174](adr/0174-external-database-driver-protocol.md),
> [ADR-0175](adr/0175-first-party-database-driver-programs.md),
> [ADR-0190](adr/0190-oracle-through-sqlcl-the-user-installs.md)

A database the IDE does not compile in is served by a **driver program**: any executable, written
in any language, that the IDE starts and talks to over its standard input and output. A plug-in
ships the program and declares the driver in its `plugin.json`; every Zen IDE then lists it in the
connection dialog next to the built-in drivers, and the Database workspace (navigator, consoles,
results, editing, export, history, production safety) works with it through the capabilities it
declares.

This page is the contract for driver authors. The IDE's side is `zen_db::external`; the reference
driver is `crates/platform/db/reference-driver/main.rs` (an in-memory SQL database, about a
thousand lines of Rust with only `serde_json`); the conformance suite is `crates/platform/db/tests/external_driver.rs`.

## Running the conformance suite against your driver

```sh
ZEN_DB_DRIVER=/path/to/your-driver \
ZEN_DB_DRIVER_ID=your-id \
ZEN_DB_DRIVER_ARGS='["--flag"]' \
ZEN_DB_DRIVER_CONNECT='{"host":"localhost","port":5000,"database":"test","user":"me","settings":{}}' \
ZEN_DB_DRIVER_PASSWORD='…' \
ZEN_DB_DRIVER_QUERY='SELECT 1' \
ZEN_DB_DRIVER_SLOW='SELECT pg_sleep(10)' \
cargo test -p zen-db --test external_driver
```

`ZEN_DB_DRIVER_QUERY` must return at least one row; `ZEN_DB_DRIVER_SLOW` must run for several
seconds and be cancellable (leave it empty to skip the cancel test). Tests named `reference_only_…`
check the IDE's own behaviour with the reference driver's test statements and pass at once for
another program. Without `ZEN_DB_DRIVER`, the suite runs the reference driver.

## The program

- It is started with the manifest's `args` (never secrets) in its own folder, with
  `ZEN_DB_DRIVER_PROTOCOL=1` in its environment and the user's environment otherwise.
- **Framing:** one JSON-RPC 2.0 message per line, UTF-8, ended by `\n`, on standard input (IDE to
  driver) and standard output (driver to IDE). A message must not contain a raw newline (JSON
  escapes it in strings). A line is at most 64 MiB; the IDE stops a driver that sends a longer one.
  Nothing but messages may be written to standard output.
- **Standard error** is free text for people: the IDE keeps its last lines in the driver's log
  (`db.driver.log`) and shows the last few when the program stops unexpectedly, with passwords
  removed.
- When its standard input closes, the program exits.
- It may serve several sessions at once and must answer `cancel` and `ping` while a statement runs;
  answers may come in any order and are matched by `id`. Requests of one session may be handled one
  at a time.

## Messages

Requests carry `"jsonrpc": "2.0"`, a numeric `id`, `method` and `params` (an object). Answers carry
the same `id` and either `result` or `error`. A notification (no `id`) from the driver may be `log`
`{ "level": "info" | "warning" | "error", "message": … }`; the IDE adds it to the driver's log. The
IDE never sends the driver requests it must answer other than those below, and answers any request
from the driver with `-32601`.

### Errors

```json
{ "code": -32000, "message": "relation \"t\" does not exist",
  "data": { "kind": "server", "code": "42P01", "position": 15 } }
```

| Code | Meaning |
|---|---|
| `-32700` | The line is not JSON (answer with `id: null` and go on) |
| `-32600` | Not a valid request, or a request before `initialize` |
| `-32601` | **Unknown method**: every method the driver does not have must be answered with this |
| `-32602` | Invalid parameters |
| `-32603` | An internal error of the driver |
| `-32000` | A database error; `data.kind` says which |
| `-32010` | The protocol version asked for is not supported (`data.supported`: the versions it speaks) |
| `-32011` | The session named is not open |
| `-32012` | The cursor named is not open |

`data.kind` is one of `connection`, `authentication`, `tls`, `passwordNeeded`, `timeout`,
`cancelled`, `server`, `notConnected`, `invalid`, `refused`, `unsupported`; `data.code` is the
server's own code (an SQLSTATE) and `data.position` a one-based character position in the
statement. Without `data.kind`, `-32601` counts as `unsupported`, `-32011` as `notConnected`, and
anything else as `server`. A message must never contain a password; the IDE removes the
connection's password from it anyway.

### `initialize`

The first request. Until it is answered, every other request is answered with `-32600`.

```json
{ "protocol": 1, "driver": "duckdb", "client": { "name": "Zen GoGarden", "version": "26.10.3" } }
```

The result names the protocol (it must be the one asked for), the driver (its `id` must be the
manifest's) and its **capabilities**, which replace the manifest's from then on:

```json
{ "protocol": 1,
  "driver": { "id": "duckdb", "name": "DuckDB", "version": "0.3.0" },
  "capabilities": {
    "models": ["relational"], "language": "sql:duckdb",
    "transactions": true, "isolationLevels": ["serializable"],
    "catalogs": true, "schemas": true, "nodeKinds": ["catalog", "schema", "table", "view", "column"],
    "explain": false, "serverCancel": true, "bindParameters": true, "rowIdentity": "primaryKey",
    "ddlPreview": false, "serverPaging": true, "sessions": false, "readOnlySessions": true,
    "uriSchemes": ["duckdb"], "sharedConnection": false, "password": false,
    "settings": [{ "key": "file", "kind": "path" }],
    "introspection": true } }
```

Every field of `capabilities` is optional (false, empty or absent by default) except `models`
(`relational`, `keyValue`, `document`, `search`, `wideColumn`; the main one first). What the IDE
does with each:

| Capability | Effect |
|---|---|
| `language` | The query language: `sql:<dialect>` gets SQL lexing, statement splitting, production-safety judgement and generated SQL (`sql:postgres`, `sql:mysql`, `sql:mariadb`, `sql:sqlite`, `sql:clickhouse`, `sql:sqlserver` generate in that dialect; another `sql:*` gets standard SQL); anything else is a command language whose statements count as changes unless the manifest names them in `readVerbs` |
| `transactions`, `isolationLevels` | Manual commit mode, Commit/Rollback, the isolation menu |
| `serverCancel` | Cancel sends `cancel` |
| `bindParameters` | Saving edited data sends values as parameters (`execute` with `params`) |
| `rowIdentity` | Results can be edited (with `introspection` and columns that name their table); `_id` with the language `mongo` edits documents of the collection a `find` reads, saved with shell statements (docs/database-workspace.md) |
| `serverPaging` | Results are read through `query`/`fetch`; otherwise through `execute` with a row cap |
| `readOnlySessions` | Read-only connections ask the driver for read-only sessions (`readOnly` in `connect`, `apply`) |
| `password` | The connection dialog asks for a password by default |
| `settings` | Extra fields of the connection dialog (`key`, `kind`: `text`, `number`, `bool`, `path`, `choice`; `choices`; `default`); their values come back in `connect`'s `settings` |
| `introspection` | The navigator and completion read the structure with `introspect`; otherwise the tree is the driver's own (`children`) |
| `sharedConnection` | Every context shares one session (an embedded database) |

### `connect`

Opens one session. The password travels only here.

```json
{ "connection": "c-1f2e", "context": "main",
  "host": "127.0.0.1", "port": 5000, "database": "shop", "user": "ann", "password": "…",
  "url": "duckdb:///data/shop.db", "readOnly": false, "tls": { "mode": "prefer" },
  "settings": { "file": "/data/shop.db" },
  "applicationName": "Zen GoGarden", "statementTimeoutMs": 60000,
  "state": { "schema": "main", "autoCommit": true, "readOnly": false },
  "timeoutMs": 15000 }
```

`context` is `main`, `metadata` or `console:<name>`; a connection opens a session per context it
uses. `host` and `port` are already the SSH tunnel's when the connection has one. Absent fields are
not set. The result:

```json
{ "session": "s1", "serverVersion": "DuckDB 1.4.1", "inTransaction": false }
```

### `close`, `ping`, `shutdown`

- `close { session }`: closes the session; answer `{}`. The IDE does not wait for this answer.
- `ping { session }`: answers `{}` when the session still works.
- `shutdown {}`: answers `{}`; the IDE then closes standard input, and the program exits. A program
  still running two seconds later is killed.

### `execute`

Runs one statement and answers its whole result, at most `maxRows` rows.

```json
{ "session": "s1", "statement": "SELECT id, name FROM people", "maxRows": 200,
  "timeoutMs": 30000, "params": ["1", null] }
```

`timeoutMs` (optional) cancels the statement as `cancel` would when it passes. `params` (only with
`bindParameters`) are the values of the statement's placeholders in order, each as text in the
server's input format, or `null`. The result:

```json
{ "columns": [{ "name": "id", "type": "integer", "origin": "id", "source": { "table": 4012, "position": 0 } },
              { "name": "name", "type": "text" }],
  "rows": [[{ "kind": "integer", "value": 1 }, { "kind": "text", "value": "Ada" }]],
  "truncated": false, "affected": null, "inTransaction": false,
  "notices": [{ "severity": "NOTICE", "message": "…" }] }
```

`truncated` says there were more than `maxRows` rows. `affected` is how many rows a change changed.
`inTransaction` (in every answer that may change it) says whether a transaction is open. A column
names the table column it reads with `source.table` (the table's `oid` in `introspect`, any stable
number) and `origin` (the column's name); editing needs both.

### `query`, `fetch`, `closeCursor` (with `serverPaging`)

Rows are pulled page by page: the driver reads from the server only what is fetched.

- `query { session, statement, skip }` runs the statement and answers
  `{ "columns": […], "cursor": "c1", "inTransaction": … }`; `skip` rows are read and dropped first
  (the IDE reads on where it stopped). A statement without rows answers no `cursor` and its
  `affected`.
- `fetch { session, cursor, count }` answers at most `count` rows:
  `{ "rows": […], "done": false }`. With `done: true` the cursor is closed; the last page may carry
  `affected`.
- `closeCursor { session, cursor }` closes it early; answer `{}`.

### `cancel`

`cancel { session }` asks the server to stop what the session runs and answers `{}` at once; the
running request then fails with `data.kind: "cancelled"`.

### Transactions

- `apply { session, state }` sets the session's tracked state: `{ "schema", "database",
  "isolation", "autoCommit", "readOnly" }` (absent fields stay as they are); answer
  `{ "inTransaction": … }`. With `autoCommit: false`, the first change opens a transaction.
- `commit { session }`, `rollback { session }`: end the open transaction; answer
  `{ "inTransaction": false }`.

### The structure

- `children { session, path }` answers `{ "nodes": [Node] }`, the children of the node at `path`
  of the driver's own tree (the roots for `[]`). A node:
  `{ "path": ["db", "users"], "kind": "table", "label": "users", "hasChildren": true,
  "detail": "…", "system": false, "size": 8192, "rows": 2, "comment": "…" }`; `kind` is `catalog`,
  `schema`, `folder`, `table`, `view`, `column`, `index`, `keyPrefix`, `key`, `collection`,
  `searchIndex` or `{ "other": "name" }`.
- `introspect` (with `introspection`) reads the structure into the IDE's snapshot model:

  ```json
  { "session": "s1", "schemas": [], "objects": [["main", "people"]], "force": false, "system": false,
    "largeSchema": 2000, "outline": false, "fingerprints": { "shop.main": "a74396aa" } }
  ```

  answers `{ "catalogs": [Catalog], "read": ["shop.main"], "unchanged": ["shop.other"] }`. A
  catalogue is `{ "name", "current", "system", "schemasRead", "schemas": [Schema] }`; a schema
  `{ "name", "fingerprint", "level": "names" | "details", "objects": [Object] }`; an object
  `{ "kind": "table" | "view" | "materializedView" | "function" | "procedure" | "package" | "sequence" | "type",
  "name", "oid", "detailed", "rows", "size", "comment", "columns": [{ "name", "position", "typeName",
  "nullable", "default" }], "constraints": [{ "name", "kind": "primaryKey" | "unique" | "foreignKey" |
  "check", "columns", "definition", "references" }], "indexes": [{ "name", "columns", "unique",
  "primary" }], "definition" }`. Every field but `name` (and an object's `kind`) is optional.
  A schema whose fingerprint equals the one the IDE sent may be listed in `unchanged` instead of
  being read again; the IDE keeps what it had.

### Keys (key-value drivers)

`key { session, path, limit, change }` reads one key of a key-value driver's tree, after `change`
(`{ "action": "expire", "seconds": 60 }` or `{ "action": "persist" }`), as
`{ "key", "database", "type", "ttlMs", "size", "length", "encoding", "columns", "rows", "truncated" }`.

## Values

Every value is `{ "kind": …, "value": … }`:

| `kind` | `value` |
|---|---|
| `null` | absent |
| `bool` | `true` / `false` |
| `integer` | a number that fits in 64 bits |
| `decimal` | an exact number as text (`"12.50"`) |
| `float` | a number |
| `text` | a string |
| `bytes` | lowercase hexadecimal (`"00ff"`) |
| `temporal` | `{ "text": "2026-10-27 10:00:00+00", "type": "timestamptz" }` |
| `interval` | text |
| `uuid` | text |
| `json` | the JSON as text |
| `array` | a list of values |
| `map` | a list of `[key, value]` pairs (a Redis hash) |
| `document` | a list of `[field, value]` pairs in order (a MongoDB document) |
| `other` | `{ "text": …, "type": … }`, any type the protocol does not model |

## The plug-in

A plug-in declares its drivers in `plugin.json` (docs/plugins.md):

```json
"databaseDrivers": [{
  "id": "duckdb", "name": "DuckDB", "icon": "database", "protocol": 1, "process": "perConnection",
  "capabilities": { "models": ["relational"], "language": "sql:duckdb", "settings": [{ "key": "file", "kind": "path" }] },
  "readVerbs": [], "guardsReadOnly": false, "args": [],
  "platforms": {
    "macos-aarch64": { "url": "https://example.org/zen-duckdb-0.3.0-macos-aarch64", "sha256": "…" },
    "linux-x86_64": { "url": "https://example.org/zen-duckdb-0.3.0-linux-x86_64", "sha256": "…" },
    "windows-x86_64": { "url": "https://example.org/zen-duckdb-0.3.0-windows.exe", "sha256": "…", "file": "zen-duckdb.exe" }
  }
}]
```

- `id`: lowercase letters, digits, `-` and `_`; never one of a compiled-in driver (`postgresql`,
  `mysql`, `mariadb`, `sqlite`, `redis`, `clickhouse`).
- `process`: `perConnection` (one program per connection, for all of its contexts; the default) or
  `shared` (one program for every connection of the driver).
- `capabilities`: what the connection dialog shows before the program first runs; `initialize`
  should answer the same.
- `readVerbs`: for a language that is not SQL, the first words of statements that only read
  (`find`, `count`), so that read-only connections and agents limited to reading can run them.
- `guardsReadOnly`: the server itself refuses every change in a read-only session, so the IDE does
  not judge statements on a read-only connection.
- `platforms`: the program per platform (`macos-aarch64`, `macos-x86_64`, `linux-x86_64`,
  `linux-aarch64`, `windows-x86_64`): its `url`, absolute (kept as it is) or relative to the plug-in
  catalog, its `sha256`, an optional `size` in bytes (which the release server checks) and an
  optional `file` name (the URL's last part by default).

Installing or updating the plug-in from Settings → Plugins (or `plugins.install`, `plugins.update`)
downloads the program for this computer into `<plug-ins>/<plugin>/drivers/<driver>/<file>`, checks
its SHA-256 and makes it executable; one that does not match is not kept. The IDE checks the
SHA-256 again before every start; an update whose program has another SHA-256 replaces it. The
catalog is signed, so the SHA-256 is the publisher's. The update the IDE makes by itself at start skips plug-ins with
drivers: their programs are downloaded only when the user asks. Switching the plug-in off or
uninstalling it removes its drivers from the list and stops their programs.

## The IDE's side

- The program starts when the first session of a connection needs it (never when drivers are
  listed), and gets `shutdown` when its last session closes.
- `initialize` must answer within 10 s, `connect` within the connection's timeout, `ping`, `cancel`,
  `apply` and `closeCursor` within 30 s. Statements are bounded by their own timeout and Cancel.
- A program that stops by itself fails what was waiting with a lost connection; the engine
  reconnects as it does for a server that went away, starting a new program. After three stops by
  itself within a minute, the driver is not started again for that minute.
- `db.driver.list` lists external drivers after the compiled-in ones with `external`: the plug-in,
  whether the program is installed, its icon, its process model, the protocol and how many of its
  programs run. `db.driver.log { id, limit }` answers its log: programs started and stopped, standard
  error, why requests failed; never statements, rows or passwords.

## First-party driver programs

The team's own drivers are binary crates in `drivers/` ([ADR-0175](adr/0175-first-party-database-driver-programs.md)),
each declared by a plug-in in `plugins/<id>`. They depend only on `zen-db-driver-kit` (`drivers/kit`),
which runs the protocol for them: the message loop, one task per session with its inbox, `cancel`
and `timeoutMs` as one stop signal, `ping` answered at once while a statement runs, the values of
the table above, and TLS through rustls with RustCrypto in libpq's modes (`zen_db_driver_kit::tls`).
They pass the same gate as the IDE: no `ring`, `aws-lc`, OpenSSL or other C.

They are published with their plug-in ([ADR-0191](adr/0191-driver-programs-published-with-their-plugins.md)):
`cargo xtask release-build plugin <id> <version> --platform all` builds `zen-db-<driver>` for this
Mac, Linux x86_64 and Windows x86_64, signs the macOS program ad hoc, and fills the plug-in's
`platforms` in the catalog entry with each program's address on the release server
(`artifacts/plugin-<id>/<version>/zen-db-<driver>-<version>-<platform>`), SHA-256 and size; the
`platforms` in `plugin.json` stay empty for them (docs/releasing.md).

```sh
cargo build -p zen-db-sqlserver
ZEN_DB_DRIVER=$PWD/target/debug/zen-db-sqlserver ZEN_DB_DRIVER_ID=sqlserver \
ZEN_DB_DRIVER_CONNECT='{"host":"127.0.0.1","port":14333,"database":"master","user":"sa","tls":{"mode":"require"},"settings":{}}' \
ZEN_DB_DRIVER_PASSWORD='Zen-Test-2026!' ZEN_DB_DRIVER_QUERY='SELECT name FROM sys.databases' \
ZEN_DB_DRIVER_SLOW="WAITFOR DELAY '00:00:10'" cargo test -p zen-db --test external_driver
```

### SQL Server (`sqlserver`)

- **Program**: `zen-db-sqlserver` (`drivers/sqlserver`), `tiberius` without TLS features; plug-in
  `plugins/sqlserver`, language `sql:sqlserver`, default port 1433.
- **`connect`**: `host`, `port`, `database`, `user`, `password`; `settings.authentication`
  `password` (a SQL Server login) or `accessToken` (a Microsoft Entra ID token given as the
  password); `settings.strict` or `tls.direct` for TDS 8.0. `tls.mode`: `disable` sends no TLS and
  fails if the server requires it; `prefer` uses TLS when the server has it; `require`,
  `verify-ca`, `verify-full` fail without it. Without `strict`, TLS is negotiated in the pre-login
  exchange (the program sets the encryption option of tiberius' request, which tiberius built
  without TLS cannot) and the handshake travels in PRELOGIN packets, TLS 1.2 only (TDS 7.x); with
  it, TLS comes first with ALPN `tds/8.0`. With `require` the certificate is not checked but the
  handshake's signature is, also for SQL Server's own version 1 certificate. A redirection (Azure
  SQL) is followed up to three times.
- **Statements**: one batch each, with `simple_query`; an `INSERT`, `UPDATE`, `DELETE` or `MERGE`
  without `OUTPUT` runs through `sp_executesql` to answer `affected`; with `params` every statement
  does, the values bound as `NVARCHAR` in order as `@P1`, `@P2`. Only the first result of a batch is
  read. Errors carry the server's number as `code` and the start of the line it names as `position`.
- **Cursors**: `query` reads from the open TDS stream; `fetch` reads on; `closeCursor`, a `cancel`
  or another request while a cursor is open sends TDS's attention signal and reads its end, so the
  session can run the next statement (the server's end of the cancelled message and its
  acknowledgement in the next are joined for tiberius). `skip` rows are read and dropped. Errors
  of later statements of a batch are read too, after the first result.
- **Transactions**: `apply` sets `IMPLICIT_TRANSACTIONS` for `autoCommit`, `SET TRANSACTION
  ISOLATION LEVEL` (`READ UNCOMMITTED`, `READ COMMITTED`, `REPEATABLE READ`, `SNAPSHOT`,
  `SERIALIZABLE`) and `USE` for `database`; `inTransaction` is `@@TRANCOUNT > 0`, read after every
  statement; `commit` commits every open level, `rollback` rolls back.
- **Structure**: `introspect` reads the current database from `sys.*` (see the Database workspace
  document); a schema's fingerprint is its object count and latest `modify_date`. `children` walks
  databases, schemas, tables and views, and columns. A `SELECT`'s columns name their table (its
  `object_id`, as `introspect` gives it) through `sys.dm_exec_describe_first_result_set`, so results
  can be edited by primary key.
- **Values**: `tinyint` to `bigint` as integers, `real` and `float` as floats, `decimal`, `numeric`
  as exact text, `money` and `smallmoney` as floats (tiberius decodes them so; cast to `decimal(19,4)` for exact text), `bit` as booleans, texts, `uniqueidentifier`
  as UUIDs, `binary` and `varbinary` as bytes, `date`, `time`, `datetime`, `smalldatetime`,
  `datetime2` and `datetimeoffset` as temporal text in SQL Server's own form (`2026-10-27
  10:00:00.1234567 +02:00`), `xml` as other.

### MongoDB (`mongodb`)

- **Program**: `zen-db-mongodb` (`drivers/mongodb`), a client of its own on `bson` (ADR-0175,
  decision 5); plug-in `plugins/mongodb`, model `document`, language `mongo`, default port 27017.
  MongoDB 3.6 and newer (OP_MSG).
- **`connect`**: `host` and `port`, or `url` (`mongodb://user:password@a:27017,b:27018/db?…` or
  `mongodb+srv://cluster.example.net/db`); the dialog's `user`, `password`, `database` and `tls`
  win over the address's. Options read from the address or `settings`: `authSource` (default
  `admin`), `authMechanism` (`SCRAM-SHA-256`, `SCRAM-SHA-1`; by default what `hello`'s
  `saslSupportedMechs` offers; `MONGODB-X509` logs in on `$external` with the TLS client
  certificate, as the user given or the certificate's subject, and is refused without TLS and a
  client certificate), `replicaSet`, `directConnection`, `appName`, `tls`/`ssl`,
  `tlsAllowInvalidCertificates` and `tlsInsecure` (mode `require`), `tlsAllowInvalidHostnames`
  (`verify-ca`), `tlsCAFile`, `tlsCertificateKeyFile`. TLS runs from the first byte; `prefer` tries
  it and falls back to plain TCP. `mongodb+srv://` looks up `_mongodb._tcp.<host>` (SRV; targets
  outside the host's parent domain are dropped) and the host's TXT record (`authSource`,
  `replicaSet` only), turns TLS on, and asks the DNS servers of `/etc/resolv.conf`. Without
  `directConnection` a replica-set member that is not primary leads to the primary its `hello`
  names.
- **Statements**: a subset of the MongoDB shell, read by the program, one per execute:
  `db.<collection>.find(filter, projection)` with `.sort()`, `.limit()`, `.skip()`,
  `.projection()`, `.hint()`, `.collation()`, `.batchSize()`, `.maxTimeMS()`, `.comment()`,
  `.count()`, `.explain()`; `findOne`, `aggregate`, `countDocuments`, `estimatedDocumentCount`,
  `count`, `distinct`, `insertOne`, `insertMany`, `updateOne`, `updateMany`, `replaceOne`,
  `deleteOne`, `deleteMany`, `findOneAndUpdate`, `findOneAndReplace`, `findOneAndDelete`,
  `createIndex`, `createIndexes`, `dropIndex`, `dropIndexes`, `getIndexes`, `drop`,
  `renameCollection`, `stats`; `db.runCommand()`, `db.adminCommand()`, `db.getCollectionNames()`,
  `db.getCollectionInfos()`, `db.createCollection()`, `db.createView()`, `db.dropDatabase()`,
  `db.aggregate()`, `db.stats()`, `db.serverStatus()`, `db.version()`, `db.currentOp()`;
  `db.getCollection('name')`, `db['name']`, `db.getSiblingDB('other')`; `show dbs`,
  `show collections`, `use <database>`; and a bare command document (`{ ping: 1 }`). Values are
  written as in the shell: unquoted keys, `'…'` and `"…"`, `ObjectId()`, `ISODate()`, `new Date()`,
  `NumberLong()`, `NumberInt()`, `NumberDecimal()`, `UUID()`, `BinData()`, `Timestamp()`,
  `/regex/flags`, `MinKey`, `MaxKey`, comments; a document that is one Extended JSON wrapper
  (`{"$oid": …}`, `{"$date": …}`, `{"$numberLong": …}`, `{"$binary": …}` …) is read as the value it
  stands for, so JSON the IDE writes back keeps its types. A statement it cannot read fails with kind `invalid`
  and the position where reading stopped. The IDE splits consoles as it splits SQL
  (`Dialect::Script`): scripts on `;`, Execute Statement also at blank lines, so a statement may
  span lines (`.sort()` on the next line included), with JavaScript's quotes and comments.
- **Results**: documents are rows; the columns are the top-level fields of the first batch in the
  order they appear (`_id` first), typed by their BSON type (`mixed` when it varies); a field a
  document lacks is null; fields that appear only in later batches are not shown. Nested documents
  and arrays are `json` in relaxed Extended JSON; `ObjectId`, `Timestamp`, regular expressions,
  `MinKey`/`MaxKey` are `other` with the shell's text; dates are temporal (`1815-12-10T00:00:00Z`,
  type `date`); `Decimal128` is decimal; UUID binaries are UUIDs. Changes answer `affected`, the
  reply's `n` (for updates the documents matched, upserts included, so the IDE's saves find a
  document whose value they set to what it was); other commands answer their reply as one row without `ok` and the
  cluster time; `distinct` answers one column; counts one row `count`.
- **Cursors**: `query` runs `find` or `aggregate` with a first batch of 101 and answers the columns;
  `fetch` takes the buffered documents and sends `getMore` with the page size; `closeCursor`, a
  cut `execute` and the end of a session send `killCursors`. `skip` documents are read and dropped.
- **Cancel and timeouts**: `timeoutMs` becomes the command's `maxTimeMS` as well; `cancel` (or the
  timeout passing first) closes the connection, opens a new one and kills with `killOp` the
  operations `$currentOp` lists under the old connection's id. The session goes on in the new
  connection, in the same database; open cursors are gone.
- **Read-only sessions**: with `readOnly` the program refuses every statement that may write
  (inserts, updates, deletes, index and collection changes, `$out` and `$merge`, and any command
  outside a list of reading ones) with kind `refused`; the plug-in declares `guardsReadOnly`.
- **State and transactions**: `apply` takes `database` (or `schema`), `readOnly` and `autoCommit`.
  With `autoCommit: false` the first statement starts a transaction in a logical session of the
  program's own (`lsid`, `txnNumber`, `startTransaction`, `autocommit: false` on every command and
  `getMore` until it ends); `commit` and `rollback` send `commitTransaction` and
  `abortTransaction`. MongoDB has transactions on replica sets and sharded clusters only; a
  standalone server refuses them with its own error. Cancel, a timeout or a lost connection ends
  the transaction (the server aborts it). Saving grid edits never starts one of its own.
- **Editing**: `rowIdentity` is `_id`; the IDE saves grid edits with `deleteOne`, `insertOne` and
  `updateOne` statements on `db.getSiblingDB(…).getCollection(…)` and reads a new document's `_id`
  back with a `find` projected on it.
- **Structure**: `children` answers databases (`listDatabases` with `authorizedDatabases`, sizes,
  `admin`, `local`, `config` as system), then collections and views (`listCollections`, views with
  their source), then indexes (`listIndexes`, the keys and `unique` as detail). No `introspect`.
- **Not yet**: compression, AWS, LDAP and Kerberos logins,
  load balancers, reading from secondaries, retryable writes, explain plans in the plan view, and `mongodb+srv://` on Windows (no
  `/etc/resolv.conf`).

### DuckDB (`duckdb`)

- **Program**: `zen-db-duckdb` (`drivers/duckdb`); plug-in `plugins/duckdb`, model `relational`,
  language `sql:duckdb` (standard SQL splitting and production safety), no port. DuckDB is C++, so
  the program drives the **`duckdb` command-line program** the user installs (ADR-0175, decision 6),
  one process a session, fed over its standard input, never through a shell: the setting
  `program`, else `duckdb` on `PATH`, `~/.duckdb/cli/latest`, `/opt/homebrew/bin`,
  `/usr/local/bin`, `/usr/bin`; a missing program fails with kind `invalid` and code
  `duckdbMissing`.
- **`connect`**: the setting `file` (else a `duckdb:` address's path, else `database`) is the
  database file; none is an in-memory database. A read-only connection opens a file with
  `-readonly`. `state` (`database`, `schema`, `autoCommit`) is applied at once (`USE`).
- **Statements**: one statement an `execute`, checked by the program first: a dot-command, a second
  statement or an open quote or comment is refused with kind `invalid` and the place, so nothing
  can reach the shell (`.shell`, `.system`). A query (`SELECT`, `WITH`, `FROM`, `VALUES`, `TABLE`,
  `SHOW`, `DESCRIBE`, `SUMMARIZE`, `PIVOT`) is described first (`DESCRIBE …`) for its column names and
  types, so an empty result has its columns; rows are read in `.mode jsonlines` with repeated column
  names kept, at most `maxRows`. Parquet, CSV and JSON files are read by DuckDB itself
  (`SELECT * FROM 'orders.parquet'`, `read_csv(…)`). Errors keep DuckDB's message, and the `LINE n:`
  context with its caret becomes the position.
- **Values**: integers, `HUGEINT` and `DECIMAL` as decimals, floats (`NaN`, infinities), text,
  dates and times as temporal with their type, intervals, UUIDs, `BLOB` as bytes, `JSON`, and
  lists, arrays, structs, maps and unions as JSON.
- **Transactions**: `BEGIN`, `COMMIT`, `ROLLBACK` as statements and as `commit`/`rollback`;
  with `autoCommit: false` the first change opens one.
- **Cancel and timeouts**: SIGINT, which ends `duckdb` reading a pipe together with its statement;
  the next request starts the program again in the same database and schema. An in-memory
  database loses its data then, and an open transaction is gone. Not on Windows (no SIGINT): the
  program is ended.
- **Structure**: `children` answers databases (`duckdb_databases()`, attached files with their
  path, `system` and `temp` as system), schemas, tables (estimated rows, comments) and views,
  then columns with their type and `NOT NULL`. No `introspect`; the IDE opens a table or view of the
  tree as a console with `SELECT * FROM "db"."schema"."table"`.
- **Not yet**: editing results (no `rowIdentity`: the program would need introspection and column
  sources), reading results page by page (`serverPaging`), and Cancel without ending the process.

### Oracle (`oracle`)

- **Program**: `zen-db-oracle` (`drivers/oracle`); plug-in `plugins/oracle`, model `relational`,
  language `sql:oracle` (scripts split by `Dialect::Oracle`, generated SQL in `SqlDialect::Oracle`),
  port 1521. No client of Oracle's protocol exists in pure Rust, so the program drives **SQLcl**,
  which the user installs, one process a session, fed over its standard input, never through a shell
  ([ADR-0190](adr/0190-oracle-through-sqlcl-the-user-installs.md)). It is started as
  `sql -S -R 3 -NOLOGINTIME /NOLOG` in the temporary folder, with `JAVA_HOME` from the setting
  `javaHome` and `TNS_ADMIN` from `tnsAdmin`. SQLcl is the setting `program` (the program or its
  folder), else `$SQLCL_HOME/bin`, `$ORACLE_HOME/sqlcl/bin`, `PATH`, `/opt/homebrew/bin` and
  `/usr/local/bin`. A missing SQLcl fails with kind `invalid` and code `sqlclMissing`; a SQLcl that
  ends because it finds no Java, or one too old, with code `javaMissing`.
- **`connect`**: the settings `connectionType` (`serviceName`, `sid`, `tns`, `custom`), `tnsAlias`,
  `tnsAdmin`, `connectString`, `role` (`normal`, `SYSDBA`, `SYSOPER`, `SYSDG`, `SYSBACKUP`, `SYSKM`)
  and `wallet` make the connect identifier: `//host:port/service`, `host:port:SID`, the alias, or the
  string as written; with `tls.mode` `require`, `verify-ca` or `verify-full` a TCPS descriptor with
  `SSL_SERVER_DN_MATCH` on only for `verify-full` and a wallet folder as `MY_WALLET_DIRECTORY`; a
  wallet `.zip` is opened first with `SET CLOUDCONFIG`. `CONNECT user/"password"@identifier` is
  written on SQLcl's standard input; every line SQLcl prints has the password replaced before it is
  kept. A value that would leave its line (a line break, a double quote in the password, spaces or
  parentheses in an identifier) is refused. After connecting: `SQLFORMAT JSON`, `DEFINE OFF`,
  `SQLBLANKLINES ON`, `SQLTERMINATOR OFF`, `EXITCOMMIT OFF`, `AUTOCOMMIT` as `state.autoCommit`, ISO
  date formats (`ALTER SESSION SET NLS_…`), the client's name as the session's module, and `state`
  (`schema` as `CURRENT_SCHEMA`, `isolation`).
- **Statements**: one statement an `execute`, sent as SQLcl's buffer and `/`, then `PROMPT` of a
  random marker. Only SQL and PL/SQL go: a first word that is not one (`HOST`, `@`, `SET` other than
  `SET TRANSACTION`/`ROLE`/`CONSTRAINTS`, `SPOOL`, `CONNECT`) or a line SQLcl acts on (`/` or `.`
  alone, `#` first) is refused with kind `invalid` and the place. A SQL statement loses its final
  `;`, a PL/SQL block or unit keeps it. Before a query, `SET ROWLIMIT maxRows + 1`; its rows are
  read from the JSON document (`results[0].columns` with Oracle types, `items` by place, or by name
  when SQLcl leaves fields out), numbers as their exact text. Feedback lines give `affected`; other
  lines (`Table T created.`) come back as `notices`. An error report gives the message, `data.code`
  (`ORA-…`, `PLS-…`, `SP2-…`) and `data.position` from `Error at Command Line : L Column : C`,
  `ORA-06550: line L, column C` or a compiler log's `L/C`; `ORA-01013` is `cancelled`, `ORA-01017`
  and account errors `authentication`, `ORA-12…`, `TNS-`, `IO Error` and lost connections
  `connection` (SQLcl is then started again), certificate and SSL errors `tls`. A statement keeps at
  most 64 MiB of output: a query beyond it is read again with half the rows that fitted and answered
  `truncated`; another statement beyond it fails. SQLcl runs in a new 0700 folder of its own,
  removed when it ends, without `SQLPATH` or `ORACLE_PATH`, so no `login.sql` but the session's runs.
- **Values**: `NUMBER` and its kin as integers when they fit, else decimals; `BINARY_FLOAT`/`DOUBLE`
  as floats; `DATE` and `TIMESTAMP…` as temporal with their type; `INTERVAL…` as intervals; `RAW`
  and `BLOB` as bytes; `JSON` as JSON; `BOOLEAN`; character types and `CLOB` (up to `SET LONG`, a
  million characters) as text; others as `other`. An empty string is `NULL`, as Oracle has it.
- **Transactions**: `autoCommit: false` is `SET AUTOCOMMIT OFF`; a change, a `SELECT … FOR UPDATE`,
  `SET TRANSACTION` or a PL/SQL block opens a transaction; a definition, `COMMIT` and `ROLLBACK` end
  it. The engine's `BEGIN` (saving edits) turns autocommit off until the next `COMMIT` or
  `ROLLBACK`; `RELEASE SAVEPOINT` is answered without running. Isolation levels `READ COMMITTED`
  and `SERIALIZABLE` (`ALTER SESSION SET ISOLATION_LEVEL`).
- **Cancel and timeouts**: SIGINT to SQLcl's process group; when SQLcl has not answered three
  seconds later it is killed, and the next request starts and connects it again in the session's
  state (an open transaction is gone). Not on Windows (no SIGINT): SQLcl is ended.
- **Structure**: `introspect` reads `ALL_USERS` (Oracle-maintained users as system schemas, the
  current schema never), `ALL_OBJECTS` (tables, views, materialised views, packages, procedures,
  functions, sequences, types; generated and recycle-bin objects left out), `ALL_TAB_COLUMNS`
  (types written as Oracle writes them, identity columns), `ALL_CONSTRAINTS` and
  `ALL_CONS_COLUMNS` (keys, foreign keys, checks other than `NOT NULL`), `ALL_INDEXES`,
  `ALL_TAB_COMMENTS`, `ALL_COL_COMMENTS`, `ALL_TABLES` (estimated rows), `ALL_VIEWS`, `ALL_SOURCE`
  and `ALL_SEQUENCES`; a schema's fingerprint is its objects' count and latest `LAST_DDL_TIME`.
  `children` answers schemas, the tables and views of one and their columns. The catalogue is the
  container (`CON_NAME`) or the database. Needs Oracle 12.2 or newer.
- **Editing**: a query `SELECT … FROM [schema.]table [alias] [WHERE …] [ORDER BY …]` gets each
  column's `origin` and `source.table` (the table's `OBJECT_ID`) from one look at `ALL_OBJECTS`
  and `ALL_TAB_COLUMNS`, so the result is edited by its primary key; values are written as literals
  (`bindParameters` is false).
- **Not yet**: `serverPaging`, bind parameters, `DBMS_OUTPUT`, `explain`, a CA file and client
  certificate from `tls` (Java's trust store or a wallet instead; `require` checks the certificate as
  `verify-ca` does), triggers, synonyms and database links in the tree, and Cancel without ending
  SQLcl on Windows.

```sh
cargo build -p zen-db-oracle
ZEN_DB_DRIVER=$PWD/target/debug/zen-db-oracle ZEN_DB_DRIVER_ID=oracle \
ZEN_DB_DRIVER_CONNECT="{\"user\":\"zen\",\"database\":\"FREEPDB1\",\"settings\":{\"program\":\"$PWD/drivers/oracle/tests/fake/sql\"}}" \
ZEN_DB_DRIVER_PASSWORD=secret ZEN_DB_DRIVER_QUERY='SELECT * FROM people' \
ZEN_DB_DRIVER_SLOW='BEGIN DBMS_SESSION.SLEEP(30); END;' cargo test -p zen-db --test external_driver
```

The fake SQLcl (`drivers/oracle/tests/fake/sql`, a shell script) answers the statements the
driver's tests send in the output format SQLcl documents; with it the suite needs neither Oracle
nor Java. Against a real server, point `program` at a real SQLcl and `ZEN_DB_DRIVER_CONNECT` at it
(docs/development.md, "Database integration tests").
