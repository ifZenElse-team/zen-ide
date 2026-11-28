# Development setup

> **Last updated:** 2026-11-11

## Toolchain

Zen IDE builds with the latest stable Rust only ([ADR-0013](adr/0013-msrv-latest-stable.md)).

1. Install Rust with [rustup](https://rustup.rs). A Rust installed by an operating system package
   manager (for example Homebrew) is not supported: it may lag behind stable and cannot install the
   WebAssembly target.
2. Clone the repository and enter it. `rust-toolchain.toml` makes rustup install the stable
   toolchain with `rustfmt`, `clippy` and the `wasm32-unknown-unknown` target automatically.
3. Keep the toolchain current: `rustup update`.
4. Install the one extra tool the quality gate needs: `cargo install --locked cargo-deny`.

## Everyday commands

| Command | What it does |
|---|---|
| `cargo build` | Build the workspace |
| `cargo test` | Run the tests |
| `cargo xtask` | List the automation tasks |
| `cargo xtask ci` | **The quality gate.** Run it before every commit you push |
| `cargo xtask release ides <YY.M.N>` | Build and publish every IDE at one calendar version (ADR-0146); `release toolbox <version>`, `release plugin <id> <version>` ([releasing.md](releasing.md)) |
| `cargo xtask release-version` | The IDEs' next calendar version, e.g. `26.10.1` |
| `cargo xtask release-ci plan [--dry-run]` | What a release of this commit would build at the calendar's next version, and what would refuse it; `matrix`, `gate`, `part`, `join`, `stage`, `verify`, `notes` and `live` are the jobs of the release workflow ([ADR-0301](adr/0301-releases-start-from-a-release-branch.md), [releasing.md](releasing.md)) |
| `cargo xtask changelog [--json <file>]` | Check the changelogs and that every decision record has its own number; write them as JSON for the website ([changelog/README.md](../changelog/README.md)) |
| `cargo xtask changelog preview [--out <file>]` | Print the notes waiting in `changelog/unreleased/` for the next release, by product and section; the workflow "Next release" writes them into a draft release after every merge into `main` |
| `cargo xtask icon-import <export.svg> <product>` | Import a product icon exported from Figma |
| `cargo xtask bundle-plugins <product> <folder>` | Lay out an IDE's bundled plug-ins; `ZEN_BUNDLED_PLUGINS=<folder> cargo run -p go-garden` then starts with them, and `ZEN_DEV_AREAS=rust,markdown` makes some of them the product's areas, offered on the first start ([plugins.md](plugins.md#in-the-ide)) |
| `cargo xtask mascot` | Write the website's pictures of Kozō (`deploy/releases/site/img/kozo/`) from the sprites in `zen-mascot` ([ADR-0125](adr/0125-kozo-the-family-mascot.md)); a test fails while they are out of date |
| `cargo xtask site-docs` | Build the website's documentation from [docs/guide](guide/README.md) and the keymap |
| `cargo xtask bench-editor` | Measure typing and scrolling frame times on large Go files ([performance.md](performance.md)) |
| `cargo xtask dev-server` | A local release server with test releases for Zen Toolbox ([release-manifest.md](release-manifest.md)) |
| `cargo xtask layers` | Check the layering rules ([ADR-0014](adr/0014-refined-layer-model.md)) |
| `cargo xtask comments` | Check that Rust sources have no comments ([ADR-0194](adr/0194-comments.md)) |
| `cargo xtask legal` | Check the change against the clean-room rules ([ADR-0280](adr/0280-clean-room-process.md)); `--all` lists every finding in the tree |

## The quality gate

`cargo xtask ci` runs these steps, all of them even if one fails, and prints a summary:

| Step | Command | Fails when |
|---|---|---|
| format | `cargo fmt --all --check` | Code is not formatted; fix with `cargo fmt --all` |
| clippy | `cargo clippy --workspace --all-targets -- -D warnings` | Any lint warning |
| test | `cargo test --workspace` | Any failing test |
| layers | `cargo xtask layers` | A layering rule is broken ([ADR-0014](adr/0014-refined-layer-model.md)) |
| comments | `cargo xtask comments` | Any comment other than `// SAFETY:`, a lone one-line `// WHY:`, or the docs of a command schema or clap type ([ADR-0194](adr/0194-comments.md)) |
| legal | `cargo xtask legal` | A line or file this change adds (since the merge base with `origin/main`, or the base CI names; uncommitted and untracked files included) names another product, uses reference wording (a parity claim, "copied from" or "ported from" a named source, another tracker's issue id); an image is added outside `assets/`, `deploy/releases/site/img/` and our own renders; a file name names a product; a line looks like a secret; or a commit since the base, written from 2026-11-12 on, has no `Signed-off-by` ([ADR-0280](adr/0280-clean-room-process.md), AGENTS.md golden rule 14). The product names themselves are not in the repository: the step reads them from the file the environment variable `ZEN_LEGAL_WORDS` names (the maintainers keep the list), one entry per line: a name, `neutral: <text>` for text that holds a name and is not one (a font, a package, a protocol request), `context: <text>` for a phrase that makes a line a compatibility line, `issue: <regex>` for another tracker's issue ids; `#` starts a comment. Without the variable the names, file names and issue ids are not checked, the step says so and the other checks still run. The allowlist of paths (import and compatibility features, legal notices, accepted ADRs up to 0280), each entry with its reason, and the compatibility contexts that name no product are in `xtask/src/legal.rs`. Findings in the rest of the tree are only counted while the cleanup cards are open; `cargo xtask legal --all` lists them and fails |
| i18n | `cargo xtask i18n` | A translation lacks a message, has an extra one, or uses other variables; non-test code asks with `t!("id")` for a message no English file has; a computed `t!(&format!(…))` is not checked and is counted in the output ([ADR-0024](adr/0024-localisation.md)) |
| changelog | `cargo xtask changelog` | A changelog is malformed, a plug-in's version has no notes ([changelog/README.md](../changelog/README.md)), or two decision records in `docs/adr` share a number |
| deny | `cargo deny check` with `deny.toml` | A disallowed licence, a security advisory, or an unknown source ([ADR-0002](adr/0002-open-source-clean-room-pure-rust.md)) |
| wasm | `cargo build --target wasm32-unknown-unknown` for every crate below L6, warnings denied | The core stops compiling for WebAssembly, or compiles with warnings |

**A tree that passed is not run again** ([ADR-0141](adr/0141-the-gate-remembers-a-pass-by-its-tree.md)).
When the worktree is clean (no changes, no untracked files), a pass is recorded as
`<target>/gate-passed/<tree hash>` with the `rustc -V` it passed with; `cargo xtask ci` on the same
tree (`git rev-parse HEAD^{tree}`: every committed file, `Cargo.lock` included) with the same
toolchain prints that it already passed and runs nothing. `cargo xtask ci --force` runs it anyway,
for example to see a security advisory published since (`deny` is part of the remembered pass). A
worktree with changes always runs and records nothing, and so does one that changed while the gate
ran. The release commands use the same record, so several of them on one tree run the gate once.

`--skip <step>[,<step>...]` and `--only <step>[,<step>...]` run some of the steps (the names of the
table above; an unknown one is an error listing them), for example `cargo xtask ci --skip test` for
the fast checks and `cargo xtask ci --only test` for the tests, as CI's two stages do. The summary
lists the steps left out. Such a run is never skipped and never remembered: only a full run is.

CI runs the same command on every pull request, in two stages: the fast checks (`--skip test`),
then the tests (`--only test`) once they pass. **A pull request merges only when both pass.**
Before pushing, run `cargo xtask ci --skip test` (format, Clippy, layers, comments, legal,
translations, changelogs, deny, WebAssembly: a minute or two) and `cargo test -p <crate>` for every
crate you changed; the full gate may be left to CI. Releases are made by the maintainers
([releasing.md](releasing.md)).

The maintainers also run the whole gate on `main` every night, several times in a row with
`--force`: it finds tests that fail now and then, and security advisories published after a
remembered pass.

### Real JVM tools

Some tests are ignored by default because they need real tools: the ignored tests of
`zen-workbench` need a real JDK, Maven, Gradle and JaCoCo, and those of `java_language_server.rs`
and `java_debug_and_tests.rs` need Eclipse JDT LS and java-debug. Each of them fails on a missing
variable instead of passing. The maintainers run them every night; to run them yourself, set the
variables below and run `cargo test -p zen-workbench --lib -- --ignored <test>`.

| Variable | Set to |
|---|---|
| `ZEN_JDK_HOME`, `ZEN_APP_JDK_HOME`, `ZEN_GRADLE_JVM`, `ZEN_TEST_JDK` | The JDK's home |
| `ZEN_MAVEN`, `ZEN_GRADLE` | The `mvn` and `gradle` programs |
| `ZEN_TEST_MAVEN`, `ZEN_TEST_GRADLE` | The folders that hold them |
| `ZEN_TEST_BUILD_TOOLS` | One folder with both |
| `ZEN_JACOCO_HOME` | A folder with the pinned jars under `.m2/repository/org/jacoco` |
| `ZEN_TEST_MAVEN_REPO` | Maven's repository for these tests |
| `ZEN_TEST_GRADLE_ZIP`, `ZEN_TEST_GRADLE_SHA256`, `ZEN_TEST_GRADLE_VERSION` | A Gradle distribution, its checksum and version |
| `ZEN_JDTLS` | The JDT LS folder, with java-debug under `bundles/` |
| `ZEN_JDTLS_JAVA_HOME`, `ZEN_PROJECT_JAVA_HOME` | The JDK JDT LS runs on, and the one its projects use |
| `ZEN_MAVEN_HOME`, `ZEN_GRADLE_HOME` | The Maven and Gradle homes |
| `ZEN_MAVEN_REPO`, `ZEN_GRADLE_USER_HOME` | A folder holding Maven's `repository`, and Gradle's user home |

The nightly run uses Temurin 25.0.5, Maven 3.9.16, Gradle 9.8.2, kotlin-compiler 2.4.21, JaCoCo
0.8.16 and JDT LS 1.63.0 with java-debug 0.53.1, each checked against its publisher's checksum. The
tests download Maven plug-ins and the libraries of both build tools from `repo.maven.apache.org`
into the folders the variables name, not into `~/.m2` or `~/.gradle`; they also reach
`plugins.gradle.org` for the Kotlin plug-in, and one test downloads JDT LS from
`download.eclipse.org`.

The Node.js debugger tests are ignored too, because they need Node.js:
`cargo test -p zen-workbench --lib nodejs::adapter -- --include-ignored`.

Linux and Windows release builds ([ADR-0150](adr/0150-linux-and-windows-builds-on-the-mac-runner.md))
are made on a Mac with `brew install zig mingw-w64`, `cargo install --locked cargo-zigbuild` and
`rustup target add x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-pc-windows-gnu`:
`cargo xtask release-build ides <version> --platform linux` (or `windows`, `all`).
Every cargo invocation uses `--locked`: commit `Cargo.lock` together with dependency changes.

Tests pass on a busy machine too: a test checks what it means without the clock
(work counted, a server that never answers, a gate the test opens), or uses a generous ceiling far
from what a broken build would take (a minute for what must not hang). Wall-clock budgets of speed
(a frame in 250 ms, a search in 50 ms) are run by hand: set `ZEN_TIMING_CHECKS=1`, or run the
ignored timing tests (`cargo test --release -p <crate> -- --ignored`).

Tests pass on a machine in any language: a crate whose tests show interface text turns on
`zen-i18n`'s `thread-languages` feature in its `[dev-dependencies]`. Each test thread then has its
own interface language, English until it chooses another with `zen_i18n::set_language`, so a test
that switches to Russian never changes another test's texts. Applications never
build with the feature.

## Database integration tests

The PostgreSQL driver (`zen-db`, ADR-0164) has integration tests against real servers in Docker.
They are `#[ignore]`d, so `cargo test` and CI never need a database; nothing connects to the
internet. Start the servers (with TLS on through the image's self-signed certificate, and `md5`
host authentication so that both md5 and SCRAM users can log in), run the tests, and remove the
containers:

```bash
for v in 15 18; do
  docker run -d --rm --name zen-pg$v -e POSTGRES_PASSWORD=zen \
    -e POSTGRES_HOST_AUTH_METHOD=md5 -e POSTGRES_INITDB_ARGS=--auth-host=md5 -p 543$v:5432 postgres:$v \
    -c ssl=on -c ssl_cert_file=/etc/ssl/certs/ssl-cert-snakeoil.pem \
    -c ssl_key_file=/etc/ssl/private/ssl-cert-snakeoil.key
done
docker exec zen-pg18 cat /etc/ssl/certs/ssl-cert-snakeoil.pem > target/zen-pg-ca.pem
ZEN_DB_PG_CA=$PWD/target/zen-pg-ca.pem cargo test -p zen-db --test postgres --test postgres_queries \
  --test postgres_transactions --test postgres_tunnel_tls --test postgres_edits --test export --test search --test compare -- --ignored --test-threads=1
cargo test -p zen-db --test structure postgresql -- --ignored
docker rm -f zen-pg15 zen-pg18
```

`ZEN_DB_PG_PORTS` (default `54315,54318`) names the ports to test; any PostgreSQL from 12 to 18
works (`postgres:14` on port 54314 with `ZEN_DB_PG_PORTS=54314,54318`). The tests cover the value
model, notices, `application_name` and the guards, cancel within a second, a killed session
reconnecting with its schema, md5 and SCRAM, TLS (require, verify-full refusing a self-signed
certificate, PostgreSQL 17+ direct TLS), the navigator tree, introspection (a schema with tables,
views, foreign keys, functions, a trigger, a policy and partitions read into a snapshot file, a
refresh after `ALTER TABLE` reading only that schema, details on demand), the PostgreSQL objects of the navigator (extensions, event triggers, roles, tablespaces, foreign tables, materialised views, schema indexes, aggregates, rules and dependencies) and the pooler option; and
(`postgres_queries.rs`) scripts with NOTICE output and results, error positions and rollback,
cancel, 100 000 rows paged within the memory budget without a lock left on the table, a portal
kept open inside a transaction, `timestamptz` in the session's zone, and read-only kept by the
server and by the engine (a write hidden in a `SELECT`, the lock while connected);
(`postgres_transactions.rs`) manual and smart commit, the transaction log, isolation and the
question before changes are lost; (`postgres_tunnel_tls.rs`) Test Connection and sessions through
an SSH tunnel (a fake `ssh` whose forward the test serves; no sshd is needed) and every TLS mode,
`verify-ca` and `verify-full` with the server's own certificate from `ZEN_DB_PG_CA` (skipped
without it).
Redis, Valkey and ClickHouse (`tests/redis.rs`, `tests/clickhouse.rs`) run the same way; the Valkey
and ClickHouse images are pulled once (`valkey/valkey:8.1-alpine`, `clickhouse/clickhouse-server:25.8`):

```bash
docker run -d --rm --name zen-redis8 -p 63808:6379 redis:8-alpine redis-server --requirepass zen
docker run -d --rm --name zen-valkey -p 63809:6379 valkey/valkey:8.1-alpine valkey-server --requirepass zen
docker run -d --rm --name zen-rcluster -p 17000-17002:17000-17002 redis:8-alpine sh -c 'cd /data; \
  for p in 17000 17001 17002; do redis-server --port $p --cluster-enabled yes --cluster-config-file n$p.conf \
  --daemonize yes --save ""; done; sleep 1; redis-cli --cluster create 127.0.0.1:17000 127.0.0.1:17001 \
  127.0.0.1:17002 --cluster-yes; tail -f /dev/null'
docker run -d --rm --name zen-rsentinel -p 16380:16380 -p 26380:26380 redis:8-alpine sh -c 'cd /data; \
  redis-server --port 16380 --daemonize yes --save ""; \
  printf "port 26380\nsentinel monitor primary 127.0.0.1 16380 1\n" > s.conf; redis-sentinel s.conf'
docker run -d --rm --name zen-ch -p 18123:8123 -e CLICKHOUSE_USER=zen -e CLICKHOUSE_PASSWORD=zen \
  -e CLICKHOUSE_DEFAULT_ACCESS_MANAGEMENT=1 clickhouse/clickhouse-server:25.8
ZEN_DB_REDIS_PORTS=63808,63809 ZEN_DB_REDIS_CLUSTER=127.0.0.1:17000,127.0.0.1:17001 \
  ZEN_DB_REDIS_SENTINEL=127.0.0.1:26380 ZEN_DB_CLICKHOUSE_PORT=18123 \
  cargo test -p zen-db --test redis --test clickhouse -- --ignored --test-threads=1
docker rm -f zen-redis8 zen-valkey zen-rcluster zen-rsentinel zen-ch
```

They cover keys grouped by the separator with `SCAN` (and never `KEYS`), each value type with its
TTL, the console's replies, cancel of a blocking command, read-only refusals, wrong passwords, a
cluster's primaries and a Sentinel's primary; for ClickHouse typed values, the tree, a streamed
result, `SET` kept in the session, cancel by `KILL QUERY`, `readonly` and `max_execution_time`.
`cargo test -p zen-workbench --lib database::tests -- --ignored` (the last port of
`ZEN_DB_PG_PORTS`) checks that a connection is read on connect and again after DDL, and that a
table's object tab reads its rows in key order with JSON, bytes, arrays and times in the value panel. `cargo test -p zen-workbench --lib database::several -- --ignored` runs one script on PostgreSQL
(the last port of `ZEN_DB_PG_PORTS`) and MySQL (the `mysql:` port of `ZEN_DB_MYSQL_SERVERS`) in
turn, each split in its dialect, and stops at the first connection that fails. The lifecycle
itself (reconnects, never replaying writes, idle close, sleep) is also tested without a server,
against the in-memory test driver (`zen_db::memory`, feature `fake`).

The MySQL and MariaDB driver has its own ignored tests (`crates/platform/db/tests/mysql.rs`) against
the official images; MySQL starts with `mysql_native_password` loaded so both plug-ins are tested,
and both servers make their own TLS certificates:

```bash
docker run -d --rm --name zen-mysql84 -e MYSQL_ROOT_PASSWORD=zen -p 127.0.0.1:33084:3306 mysql:8.4 \
  --mysql-native-password=ON
docker run -d --rm --name zen-mariadb114 -e MARIADB_ROOT_PASSWORD=zen -p 127.0.0.1:33114:3306 mariadb:11.4
# wait until both logs say "ready for connections"
docker exec zen-mysql84 cat /var/lib/mysql/ca.pem > target/zen-mysql-ca.pem
ZEN_DB_MYSQL_CA=$PWD/target/zen-mysql-ca.pem cargo test -p zen-db --test mysql --test mysql_edits --test search -- --ignored --test-threads=1
cargo test -p zen-db --test structure mysql -- --ignored
docker rm -f zen-mysql84 zen-mariadb114
```

CSV imports (`--test csv_import`) have one ignored test per server: run
`cargo test -p zen-db --test csv_import postgres -- --ignored` with PostgreSQL up and
`cargo test -p zen-db --test csv_import mysql -- --ignored` with MySQL up (the same
`ZEN_DB_PG_PORTS` and `ZEN_DB_MYSQL_SERVERS`); the SQLite one runs by default.

`tests/sessions.rs` runs against PostgreSQL, MySQL and MariaDB together (`ZEN_DB_PG_PORTS` and
`ZEN_DB_MYSQL_SERVERS`, the servers above): a transaction holding a row and a second session waiting
for it, listed with the blocking chain and the locks, the waiting statement cancelled and the
holder terminated, nothing else touched:
`cargo test -p zen-db --test sessions -- --ignored --test-threads=1`.

`ZEN_DB_MYSQL_SERVERS` (default `mysql:33084,mariadb:33114`) names the servers as `driver:port`.
They cover the value model (unsigned `BIGINT`, decimals, zero dates, bits, JSON, enums and sets,
binary strings) and warnings, the Production guards and `program_name`, `KILL QUERY` within a
second for a statement and a console, a killed session reconnecting with its database,
`caching_sha2_password` over plain TCP and `mysql_native_password`, TLS (`require`, `verify-full`
refusing the self-made certificate, `verify-ca` with `ZEN_DB_MYSQL_CA`), introspection (a database
with keys, a generated column, a view, a function, a procedure, a trigger and an event; a refresh
after `ALTER TABLE` reading only that database; names only past `large_schema`; details on demand;
`SHOW CREATE` DDL that splits back into one statement), 100 000 rows read up to the cap with
`sql_select_limit`, Fetch Next and Fetch All, error codes and positions, and manual and smart
commit following the server's transaction; and (`mysql_edits.rs`) filters, sorts with `NULL`s in
place, quick filters (`LIKE` with `%`, `_` and a backslash, bytes, booleans) and saved edits with
prepared statements, rows read back by key, an unchanged value and a conflict.

The SQL Server driver program (`drivers/sqlserver`, ADR-0175) has ignored tests
(`drivers/sqlserver/tests/server.rs`) that talk to the program as the IDE does. SQL Server's images
are for x86-64 only; on Apple silicon OrbStack or Docker Desktop runs them through Rosetta:

```bash
docker run -d --rm --name zen-mssql -e ACCEPT_EULA=Y -e 'MSSQL_SA_PASSWORD=Zen-Test-2026!' \
  -p 127.0.0.1:14333:1433 mcr.microsoft.com/mssql/server:2022-latest
cargo test -p zen-db-sqlserver --test server -- --ignored --test-threads=1
ZEN_DB_DRIVER=$PWD/target/debug/zen-db-sqlserver ZEN_DB_DRIVER_ID=sqlserver \
  ZEN_DB_DRIVER_CONNECT='{"host":"127.0.0.1","port":14333,"database":"master","user":"sa","tls":{"mode":"require"},"settings":{}}' \
  ZEN_DB_DRIVER_PASSWORD='Zen-Test-2026!' ZEN_DB_DRIVER_QUERY='SELECT name FROM sys.databases' \
  ZEN_DB_DRIVER_SLOW="WAITFOR DELAY '00:00:10'" cargo test -p zen-db --test external_driver
docker rm -f zen-mssql
```

`ZEN_DB_MSSQL_PORT` (default 14333) and `ZEN_DB_MSSQL_PASSWORD` name the server; the tests wait up
to a minute for it to accept logins. They cover typed values, TLS negotiated in the pre-login
exchange (`require`; `verify-full` refusing the image's self-signed certificate), a wrong password
and an error's number and line, 5 000 rows through a cursor with `skip`, a cursor closed early and a
cut result leaving the session usable, Cancel and `timeoutMs` on `WAITFOR`, manual commit with
rollback and commit, saved edits with `@P1` parameters and the table columns a `SELECT` names, and
introspection (identity and computed columns, keys, a foreign key with `ON DELETE CASCADE`, a check,
an index, a view's definition, an unchanged schema kept, the tree). TDS 8.0 is tested only with
`ZEN_DB_MSSQL_STRICT=1`, against a server set up with a certificate for it (SQL Server 2022 CU on
Linux, 16.0.4305, still answers "A valid TLS certificate is not configured to accept strict
connections" with a CA-signed certificate for `localhost`, so this test has not run against a
server). A server that forces encryption is tested with `ZEN_DB_MSSQL_CA` naming the CA of its
certificate: make a CA and a certificate for `localhost` signed by it, then in the container
`mssql-conf set network.tlscert …`, `network.tlskey …`, `network.forceencryption 1` (and
`network.tlsprotocols 1.2 1.3` to check that TLS 1.3 offered by the server is not used in the
pre-login), restart it, and run the tests with `ZEN_DB_MSSQL_CA=<ca.pem>`.

The MongoDB driver program (`drivers/mongodb`, ADR-0175) has ignored tests
(`drivers/mongodb/tests/server.rs`) that talk to the program as the IDE does:

```bash
docker run -d --rm --name zen-mongo -e MONGO_INITDB_ROOT_USERNAME=zen \
  -e 'MONGO_INITDB_ROOT_PASSWORD=Zen-Test-2026!' -p 127.0.0.1:27037:27017 mongo:7.0
cargo test -p zen-db-mongodb --test server -- --ignored --test-threads=1
ZEN_DB_DRIVER=$PWD/target/debug/zen-db-mongodb ZEN_DB_DRIVER_ID=mongodb \
  ZEN_DB_DRIVER_CONNECT='{"host":"127.0.0.1","port":27037,"database":"zen_test","user":"zen","tls":{"mode":"disable"},"settings":{}}' \
  ZEN_DB_DRIVER_PASSWORD='Zen-Test-2026!' ZEN_DB_DRIVER_QUERY='show dbs' \
  ZEN_DB_DRIVER_SLOW="db.slow.find({ \$where: 'sleep(100) || true' })" cargo test -p zen-db --test external_driver
docker rm -f zen-mongo
```

The image is `mongo:7.0`: `mongo:8.0` does not start on a Linux kernel 6.19 or newer (Docker
Desktop's and OrbStack's recent VMs among them), and the tests pass on both versions where 8.0 runs.
`ZEN_DB_MONGO_PORT` (default 27037) and `ZEN_DB_MONGO_PASSWORD` name the server (the conformance
run needs `zen_test.slow`, which the cancel test fills). They cover typed values and nested
documents as JSON, inserts, updates, deletes, counts and `distinct`, 5 000 documents through a
server cursor with `skip`, a cursor closed early, a cut result, Cancel (the operation gone from
`$currentOp`) and `timeoutMs` on a `$where` with `sleep`, a wrong password never echoed, a
statement the console cannot read with its position, a server error's code and a duplicate key, a
read-only session refusing changes, the navigator (databases, collections, a view, an index) with
`use` and `apply`, and a SCRAM-SHA-1 user through a `mongodb://` address. TLS is tested with
`ZEN_DB_MONGO_CA` and `ZEN_DB_MONGO_TLS_PORT` against a second server started with
`--tlsMode requireTLS --tlsCertificateKeyFile /tls/server.pem --tlsCAFile /tls/ca.pem
--tlsAllowConnectionsWithoutCertificates` (the certificate and key in one PEM file for
`localhost`, signed by the CA). The X.509 login is tested on the same server with
`ZEN_DB_MONGO_CLIENT_PEM` (a client certificate and its key in one PEM file, signed by the CA, whose
subject is `ZEN_DB_MONGO_CLIENT_SUBJECT`, by default `CN=zen,OU=clients,O=Zen Client`; the test
creates that user on `$external`). Transactions need a replica set: start
`docker run -d --rm --name zen-mongo-rs -p 127.0.0.1:27039:27017 mongo:7.0 --replSet rs0 --bind_ip_all`
and run the tests with `ZEN_DB_MONGO_RS_PORT=27039` (the test initiates the set).

The DuckDB driver program (`drivers/duckdb`, ADR-0175) drives the `duckdb` program (1.0 or newer:
it runs with `-batch -init /dev/null`); its ignored
tests (`drivers/duckdb/tests/program.rs`) take it from `ZEN_DB_DUCKDB_PROGRAM` and write their files
under `ZEN_DB_DUCKDB_DIR` (the temporary folder by default). Without DuckDB installed, a script that
runs it in Docker with that folder mounted at the same path serves:

```bash
mkdir -p /tmp/zen-duck/bin /tmp/zen-duck/data
printf '#!/bin/sh\nexec docker run -i --rm -v /tmp/zen-duck/data:/tmp/zen-duck/data duckdb/duckdb:latest duckdb "$@"\n' \
  > /tmp/zen-duck/bin/duckdb && chmod +x /tmp/zen-duck/bin/duckdb
ZEN_DB_DUCKDB_PROGRAM=/tmp/zen-duck/bin/duckdb ZEN_DB_DUCKDB_DIR=/tmp/zen-duck/data \
  cargo test -p zen-db-duckdb --test program -- --ignored --test-threads=1
ZEN_DB_DRIVER=$PWD/target/debug/zen-db-duckdb ZEN_DB_DRIVER_ID=duckdb \
  ZEN_DB_DRIVER_CONNECT='{"settings":{"program":"/tmp/zen-duck/bin/duckdb"}}' \
  ZEN_DB_DRIVER_QUERY='SELECT * FROM range(3)' \
  ZEN_DB_DRIVER_SLOW='SELECT sum(i * random()) FROM range(1000000000000) t(i)' \
  cargo test -p zen-db --test external_driver
```

They cover typed values (dates, decimals, huge integers, lists and structs as JSON, blobs),
repeated column names, the columns of an empty result, an error's position, refused dot-commands
and second statements, a database file with Parquet and CSV written and read back, the same file
read-only, transactions with manual commit, Cancel and a timeout (the process ends and the next
statement starts a new one; the file keeps its data), and the navigator with `apply`.

The Oracle driver program (`drivers/oracle`, ADR-0190) drives SQLcl. Its unit tests and
`drivers/oracle/tests/fake.rs` run everywhere against a fake SQLcl, a shell script in
`drivers/oracle/tests/fake`. The ignored tests (`drivers/oracle/tests/server.rs`) need a real SQLcl
with Java 17 or newer (`ZEN_DB_ORACLE_SQLCL`, `sql` on `PATH` by default) and Oracle Database Free
in Docker (`gvenzl/oracle-free`, about 1.5 GB; its `APP_USER` is the user the tests connect as):

```bash
docker run -d --rm --name zen-oracle -p 127.0.0.1:1521:1521 -e ORACLE_PASSWORD=Zen-Test-2026 \
  -e APP_USER=zen -e APP_USER_PASSWORD=zen_secret gvenzl/oracle-free:23-slim-faststart
ZEN_DB_ORACLE_SQLCL=/opt/homebrew/bin/sql ZEN_DB_ORACLE_PORT=1521 ZEN_DB_ORACLE_PASSWORD=zen_secret \
  cargo test -p zen-db-oracle --test server -- --ignored --test-threads=1
ZEN_DB_DRIVER=$PWD/target/debug/zen-db-oracle ZEN_DB_DRIVER_ID=oracle \
  ZEN_DB_DRIVER_CONNECT='{"host":"127.0.0.1","port":1521,"database":"FREEPDB1","user":"zen","settings":{"program":"/opt/homebrew/bin/sql"}}' \
  ZEN_DB_DRIVER_PASSWORD=zen_secret ZEN_DB_DRIVER_QUERY='SELECT * FROM all_users' \
  ZEN_DB_DRIVER_SLOW='BEGIN DBMS_SESSION.SLEEP(30); END;' cargo test -p zen-db --test external_driver
```

They cover typed values, an error's position, a SID and a connect string, PL/SQL packages and
procedures in the navigator, a compile error's place, manual commit and rollback seen from another
session, and Cancel of a sleep.

Database profiles (ADR-0173) have ignored tests of their own (`crates/platform/db/tests/profiles.rs`)
that run a profile's quirks on the stock servers above: PostgreSQL read through
`information_schema` as the CockroachDB and Redshift profiles read it (tables, views, columns and
keys; no sequence or function folders), MySQL without fingerprints and with routines and events
hidden as StarRocks, and a Redis server tested under the Valkey profile still named Redis. Run them
with the servers of the sections above (`ZEN_DB_PG_PORTS`, `ZEN_DB_MYSQL_PORT` (33084),
`ZEN_DB_REDIS_PORTS`):

```bash
cargo test -p zen-db --test profiles -- --ignored --test-threads=1
```

Servers of the profiles themselves need their own images, which are pulled only with the maintainers'
approval; until then their catalogue queries, version texts and defaults are unit-tested.

## Workspace layout

```text
crates/platform/   Zen Platform crates (zen-*), layered L0 to L5
crates/lang/       language modules (zen-lang-*)
plugins/           plug-ins (zen-plugin-*), WebAssembly modules
drivers/           database driver programs (zen-db-*) and the kit they share
apps/              applications: the IDEs, zen-toolbox, zen-releases (the release server)
changelog/         release notes of every product and plug-in
xtask/             build automation (`cargo xtask`)
docs/              documentation and decision records
```

Each directory has a README with its rules. Lints are configured once in the root `Cargo.toml`
(`[workspace.lints]`); every crate opts in with `[lints] workspace = true`. `unsafe` code is
forbidden there.

## Disk space

Builds are large: `target/` grows with every change of dependencies or versions, because Cargo keeps
the old artifacts. Debug builds carry only line tables for our crates and no debug info for
dependencies (`[profile.dev]` in `Cargo.toml`), which keeps `target/debug` several times smaller.
When `target/` grows past a few tens of gigabytes, delete it (`cargo clean`, or only `target/debug`);
everything in it is rebuilt on demand. Releases also leave `target/dist`, `target/linux` and
`target/windows`, which can go too. The Linux and Windows builders keep their Cargo registry and
toolchain in Docker volumes named `zen-linux-builder-*` and `zen-windows-builder-*`.
