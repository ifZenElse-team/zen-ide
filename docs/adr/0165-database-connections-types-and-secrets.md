# ADR-0165: Database connections: global and project scopes, connection types, settings per driver, secrets in the Keychain

- **Status:** Proposed
- **Date:** 2026-10-26
- **Relates to:** [ADR-0051](0051-jetbrains-keymaps-and-idea-project-settings.md), [ADR-0061](0061-http-client.md),
  [ADR-0122](0122-scratch-files-and-buffers.md), [ADR-0139](0139-lean-mcp-answers-secrets-and-costs.md),
  [ADR-0164](0164-database-engine-zen-db-runtime-and-drivers.md)

## Context

Before the connection registry is written, its files and its handling of secrets must be fixed,
because files shared through Git and passwords are hard to change later. Keeping connections only
per workspace or only per project does not let one connection be used in every project and also be
shared with a team in one project.

The owner (2026-10-26) wants every kind of database reachable, not only SQL ones (ADR-0164), so the
settings of a connection depend on its driver: Redis has logical databases, Sentinel and Cluster;
MongoDB has `mongodb+srv` addresses and replica sets.

## Decision

1. **Two scopes, one model.**
   - **Global connections** live in the IDE's data folder (`<data folder>/databases/data-sources.json`,
     next to the scratch files of ADR-0122) and are usable in every project of that IDE.
   - **Project connections** live in the project, **`.zen/data-sources.json`**, meant to be
     committed and shared through Git. They **never hold a secret**, and nothing personal: no
     password, no token, no user name unless the team chose to share one.
   - **Personal overrides** of a project connection (the user name, a local port, read-only, the
     connection type, whether to save the password) are kept per project in the IDE's data folder
     (`<data folder>/databases/<project key>/overrides.json`), so changing them never changes the
     shared file.
   - **Folders** group connections in both scopes (a path such as `Team/Reporting`), drawn as
     folders in the navigator; project connections come first, then global ones.
   - Both files are JSON with `"version": 1`, one entry per connection with a stable `id`, its
     `name`, `folder`, `driver`, its driver settings (decision 3), `type` (the connection type's
     id), `readOnly`, `ssh`, `tls`, navigator options and bootstrap settings (default database or
     schema, statements to run after connecting). Unknown fields are kept when the file is
     written, and the file is written only when something in it changes.
2. **Connection types.** Every connection has a type that sets its safety
   defaults. The three built-in types have these defaults:

   | Setting | Development | Test | Production |
   |---|---|---|---|
   | Auto-commit | on | on | off |
   | Confirm SQL execution (data-changing statements) | off | off | on |
   | Confirm data changes (saving edits) | off | on | on |
   | Smart commit | off | off | off |
   | Return to auto-commit after commit or rollback | on | on | off |
   | End idle transactions after | 1800 s | 900 s | 600 s |
   | Close idle connections after | 14400 s | 7200 s | 3600 s |

   New connections are Development. **Colours come from the theme** (`database.connectionType.*`
   keys of `IdePalette`, readable in light and dark themes) instead of fixed colours; a user's own type picks one of the theme's colours. People add their own types
   (Settings → Database → Connection Types), kept in `<data folder>/databases/connection-types.json`.
   A project connection naming a type the person does not have is treated as **Production** until
   they choose, so an unknown type is never the least safe one.
3. **Settings per driver kind.** The connection dialog shows the fields its driver declares
   (ADR-0164, decision 3) and accepts a pasted URI, which it splits into those fields; a password
   in the URI goes to the Keychain and is removed from the text.
   - **PostgreSQL**: host, port, database, user; `postgres://` and `postgresql://` URIs; TLS modes
     `disable`, `prefer`, `require`, `verify-ca`, `verify-full`, a CA file and a client
     certificate; **Show all databases** (on by default).
   - **MySQL and MariaDB**: host, port, database, user; `mysql://` and `mariadb://`; TLS off,
     preferred, required, or verified with a CA file.
   - **SQLite**: a file path (created only when asked), read-only, and the `sqlite3` program's path
     when it is not on `PATH`.
   - **Redis and Valkey**: `redis://` and `rediss://` (TLS) with a user (ACL), a logical database
     number and a key separator for grouping keys (`:` by default); `unix://` for a socket; the
     mode **standalone**, **Sentinel** (the master's name and the Sentinel nodes) or **Cluster**
     (seed nodes; logical databases are not offered).
   - **MongoDB**: `mongodb://` with several hosts, and `mongodb+srv://` (hosts found through DNS by
     the driver); replica set, `authSource`, `directConnection`, TLS options.
   - **ClickHouse, Elasticsearch, OpenSearch**: one or more HTTPS (or HTTP) base addresses; user
     and password, or an API key.
   - **Cassandra and ScyllaDB**: contact points, port, keyspace, local datacenter.
   - **SSH**: through the system's **OpenSSH**: a host alias from `~/.ssh/config` or a
     host, user and port; keys and the agent are ssh's own business (`IdentityFile`, `ssh-agent`,
     `UseKeychain`). The IDE forwards a free loopback port to the database (`ssh -N -L`) for the
     connection's lifetime.
4. **Secrets only in the macOS Keychain.** Passwords, SSH passwords and key passphrases asked for
   by ssh, and tokens (API keys) are stored only as Keychain items of the service
   `space.ifzenelse.zen.database`, one per connection and kind of secret (`<connection id>/password`,
   `/ssh`, `/token`; a project connection adds its project key).
   - Through **`keyring-core` 1.0 with `apple-native-keyring-store` 1.0.2** (both MIT OR
     Apache-2.0), which call the system's Security framework through `security-framework` (Rust
     bindings to a system library; nothing is compiled from C). Not the `keyring` 4.2 crate itself:
     its default features turn on stores for other systems (Secret Service over D-Bus on Linux,
     the Windows store); we choose the store per system, and those for Linux and Windows come with
     their builds.
   - **Never in files, logs, commands or MCP answers.** No command returns a secret: a connection's
     answer says `hasPassword: true` at most. Storing one goes through the connection dialog or
     `db.connection.setSecret`, which never echoes it. Logs and error messages have them removed,
     including a password inside a URI. An agent can create and use a connection but can never read
     its secret.
   - "Save password" is on by default; turned off, the password is asked at each
     connection and kept in memory only until it disconnects. When ssh asks for a passphrase, the
     IDE answers through `SSH_ASKPASS` with its own dialog, and stores the answer only when asked
     to.
5. **Import, never in place, never secrets.** **Database → Import Connections** reads:
   - **DBeaver's `data-sources.json`** from a DBeaver workspace folder the person picks: names,
     folders, drivers we have, hosts, ports, databases, user names, connection types and SSH
     settings. DBeaver's encrypted credentials file is not read.
   - **JetBrains `.idea/dataSources.xml`** (and the user names of `dataSources.local.xml`) through
     `zen-jb-import`, which never writes `.idea` (ADR-0051). Passwords stay in the JetBrains
     credential store; they are not read.
   The person chooses the scope (global or this project) and which entries to take; connections of
   drivers we do not have are listed and skipped with the reason. The other tool's files are never
   changed.
6. **Detection: offered, never connected.** When a project opens (and when the files change), the
   IDE looks for databases the project names:
   - **`.env` files**: `DATABASE_URL`, `*_DATABASE_URL`, `REDIS_URL`, `MONGODB_URI`, `MONGO_URL`,
     `CLICKHOUSE_URL`, `ELASTICSEARCH_URL`, and the `PG*`, `POSTGRES_*` and `MYSQL_*` groups;
   - **Compose files** (`docker-compose.yml`, `compose.yaml` and their variants): services whose
     image is PostgreSQL, MySQL, MariaDB, Redis, Valkey, MongoDB, ClickHouse, Elasticsearch,
     OpenSearch, Cassandra or ScyllaDB, with their published port and their environment (database,
     user).
   Each finding is shown as an offer in the Database workspace ("Detected in .env: PostgreSQL on
   localhost:5432"), with Add and Dismiss. Nothing is saved and **nothing connects** until the
   person adds it; a password found there goes to the Keychain on Add, never into a file. Agents see
   the offers without values from files hidden from them (ADR-0139). Dismissed offers are
   remembered per project.
7. **Commands** (`zen_workbench::database`): `db.connection.list`, `.get`, `.create`, `.update`,
   `.delete`, `.move` (between folders and scopes), `.test`, `.connect`, `.disconnect`,
   `.setSecret`; `db.connectionType.list`, `.create`, `.update`, `.delete`; `db.import.dbeaver`,
   `db.import.jetbrains`; `db.detect.list`, `.accept`, `.dismiss`.

## Consequences

- A team can commit its connections and each member keeps their own user name and password; a
  project's file never needs a review for leaked secrets.
- The Keychain may ask the person to allow access the first time a signed IDE reads an item; a new
  signing identity asks again.
- Linux and Windows builds need their own stores before database support is on there.
- Detection reads `.env` and Compose files, which the IDE already reads for highlighting; it adds
  no network access.
- Import covers what the other tools keep in plain files; people type their passwords once.

## Alternatives considered

- **Only project connections.** The same database
  would be configured again in every project. Rejected.
- **Only global connections.** Nothing to share with a team through Git. Rejected.
- **Secrets in our own encrypted file.** A key has to live somewhere; on macOS the Keychain is that
  place, with the system's access control. Rejected.
- **The `security` program** instead of the framework. Storing a password with it takes the
  password as a command-line argument (`-w`), which every process of the machine can read while it
  runs. Rejected.
- **Connecting detected databases automatically.** Against AGENTS.md rule 10, and a `.env` may name
  production. Rejected.
- **Reading the stored passwords of the tools we import from.** Decrypting another tool's credentials is
  not ours to do. Rejected.
