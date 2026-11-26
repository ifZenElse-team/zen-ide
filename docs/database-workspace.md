# The Database workspace

> **Last updated:** 2026-11-11 · **Decisions:** [ADR-0163](adr/0163-workspaces-full-tools-in-the-editor-area.md),
> [ADR-0164](adr/0164-database-engine-zen-db-runtime-and-drivers.md),
> [ADR-0165](adr/0165-database-connections-types-and-secrets.md),
> [ADR-0168](adr/0168-result-paging-without-held-locks.md),
> [ADR-0183](adr/0183-language-injection.md),
> [ADR-0186](adr/0186-database-keys-and-toolbar-in-the-database-workspace.md),
> [ADR-0189](adr/0189-splits-and-scripts-in-a-workspace.md) · **Menus and layout:**
> [behaviour/ide/database.md](behaviour/ide/database.md)

The Database workspace is a full database tool inside the IDE window: the **Database** button on
the right stripe expands it over the editor area, all database work happens there with tabs of its
own, and it collapses back to the code with everything left in place. Inside, it has a model of
its own; around it, the frame is the IDE's. The user's side is in the guide:
[Database workspace](guide/28-database.md).

This document is the **specification** of the workspace: what Zen IDE does, what it has today, the
stage that brings the rest, and what is left out on purpose.

## Where it is switched on

Database support is the built-in module `database` (`zen_workbench::database::MODULE`), switched on
per product as ADR-0164, decision 1, says: when it is off, no `db.*` command is registered and the
Database button is not shown; Settings → Plugins → Built-in (or `plugins.module.enable { id:
"database" }`) and a restart switch it on.

ADR-0164 has it on by default in SqlScroll and off, offered, elsewhere. **For now it is also on by
default in GoGarden and RustRonin** (the owner, 2026-10-26: to try the workspace in the IDEs people
use today) **and in WebWave** (the owner, 2026-10-28); DevopsDaruma has it on too (the owner, 2026-11-13, ADR-0285); JvmJizo carries it off. SqlScroll has no app yet. When SqlScroll ships, or the
offer of ADR-0165 (a project that names a database) lands, these defaults are revisited.

## Clean room

ADR-0002 applies. The workspace is specified in this document
and in [behaviour/ide/database.md](behaviour/ide/database.md) only, in our own words, from our own
design and the databases' public documentation. A change to the workspace's model needs a new ADR.
Non-relational databases follow the databases' own documentation.

## The workspace host (stage A)

The host is platform code that every workspace uses (ADR-0163); the Git workspace is on it too
since ADR-0187.

| Step | What | State |
|---|---|---|
| A2 | Editor groups owned by a workspace (`zen_workbench::groups`) | Built |
| A3 | Workspace state and the `workspace.*` commands (`zen_workbench::workspaces`) | Built |
| A4 | The host in the editor area, with its tab strip (`ide/workspace_host.rs`, `ide/tab_strip.rs`) | Built |
| A5 | Workspace buttons on the right stripe, Find Action | Built |
| A6 | Keys (⌘W, ⇧⎋, Switcher), refusals and `get_window_state` | Built |
| A7 | Kept across restarts in `workspace.json` | Built |
| A8 | An empty Database workspace with SQL consoles | Built |
| A9 | One front surface instead of parallel flags (`ide/front.rs`) | Built |
| A10 | The Git workspace on the host, and a tab in the editor strip for every open workspace (ADR-0187) | Built |
| A11 | Splits inside a workspace, scripts moved between it and the editor, Local History of consoles (ADR-0189) | Built |

### How it works

- **One fact decides what is in front.** Each workspace owns editor groups, kept by `EditorGroups`
  outside the main split tree in a split tree of their own (`OwnedGroup`: `root`, and `focused`, the
  one that last had the keyboard), with stable ids (from 2³² up) never renumbered; its first group is
  never removed when empty. The workspace is expanded exactly when the focused group is one of its own.
  Expanding focuses that group; collapsing focuses the main group that last had the keyboard
  (`mainFocused`); focusing any main group collapses it, so opening a code file from anywhere
  collapses the workspace and shows the file.
- **New tabs stay out of a workspace** unless the caller names its group
  (`file.document.open { group }`); a file already shown in a workspace comes to front there. A
  workspace may host only some files as text tabs (`Workspaces::host_files`; the Database workspace
  hosts `.sql` files: consoles and scripts): `workspace.tab.open` refuses any other file with
  `notHosted`, and a restore leaves such a file out, so code always opens in the editor. A
  tab is not moved between a workspace and the main editor yet: `editor.split.moveTab` and dragging
  refuse it with the reason (it may be allowed later).
  (`file.document.open { group }`); a file already shown in a workspace comes to front there.
- **Splits inside a workspace** (ADR-0189). While a workspace is expanded, the editor's split
  commands act on its area: Split Right and Split Down (the keys, the inner tab's menu,
  `workspace.tab.split`) make a new group of the workspace that shows the tab; a text tab stays in
  the old group too unless moved, a tab the feature draws always moves (refused when it is alone in
  its group). Each inner tab carries its `group`, and each group shows one of its tabs
  (`Workspaces::shown_in`); the workspace's active tab is that of the group with the keyboard.
  `workspaces::sync` follows every group, puts a feature tab whose group went into the group with the
  keyboard, and drops a group of a split workspace left without tabs (`EditorGroups::remove_owned`).
  The host lays the groups out with the main editor's layout and dividers, each with its own tab
  strip; a click gives a group the keyboard, Goto Next Splitter goes round them, a tab dragged onto
  another group moves there (`workspace.tab.move { group }`). The results panel of a console shows
  under the console with the keyboard (the feature's view keeps one results panel). The split is kept
  in `workspace.json` (`layout`, without tabs; each tab's `group`; `shown`, the tab each group showed)
  and laid out again with new ids on restore (`EditorGroups::lay_out`).
- **Scripts move between a workspace and the editor.** A feature says which project files its
  workspace takes (`Workspaces::accept_files`; the Database workspace: `.sql` files of the project,
  never consoles, `database::takes_script`). Such a file moves both ways: `editor.split.moveTab`
  into one of the workspace's groups (or `{ workspace }`), a main tab dragged onto the workspace's
  tab in the strip or, expanded, into the host; `workspace.tab.move { group }` to a main group, an
  inner tab dragged onto the editor's strip above the host, or Move to Editor in its menu. The move
  is a real move of the tab: the document, its carets and undo stay. Everything else is refused with
  code `notMovable` and the reason (`workspaces::check_move`): a console stays, a tab the feature
  draws stays, a code file is not taken, nothing goes between two workspaces.
- **Inner tabs** are of two kinds. Text tabs are documents of the workspace's group, so the editor,
  undo, find, completion and highlighting work as in code. Other tabs are drawn by the feature: the
  state keeps each one's id, kind, title, closable flag and small kind data, and the feature
  registers a `WorkspaceView` that draws them, plus a toolbar and a left panel. The state follows
  the group: a file opened into it gets a tab, a file closed elsewhere loses it, and the group's
  active file makes its tab active.
- **Consoles live in `<data folder>/consoles/`**, never in the project; closing an empty console
  deletes its file, as an empty scratch file is deleted. Local History records the folder of
  consoles as a root of its own beside the project (`local_history::place`), so every save of a
  console keeps a version and Show History works on a console's tab.
- **The host** draws the expanded workspace in place of the split editors: toolbar, left panel,
  the inner tab strip (the editor tabs' widget: icon, title, ×, middle-click close, overflow ▾,
  drag to reorder, the active tab outlined, a colour strip per tab from the feature) and the active
  tab. The strip brings the active tab into view whole whenever it changes, the strip's width
  changes (a slide, a resize, a tool window) or tabs come and go, and a scroll it makes starts the
  row at a tab, never in the middle of one (`tab_strip::revealing`). Expanding and collapsing slide from the right in about 150 ms (none with Reduce motion);
  the state and the keyboard change at the start, and the editors beneath take no pointer input
  while it slides. The split editors keep their tabs, carets and scrolling and come back as they
  were; a Git, diff, rebase or conflict tab that was in front is in front again. Distraction Free,
  Zen and Presentation modes keep an expanded workspace. AccessKit names the host
  ("Database workspace") and each tab.
- **One value says which tab without a document is in front** of the main editor (`ide/front.rs`):
  the diff or Compare Directories, with the document it came over; it gives way once a document
  comes to the front. Compare Directories stays behind a diff opened over it and is in front again
  when that diff closes.
- **An open workspace has a tab in the editor strip** (ADR-0187). A workspace is open from its
  first expansion until it is closed: the tab strip of the main group it returns to shows its tab
  (title, icon, ×) after the code's tabs, active while it is expanded, and that strip stays above
  the host while it is expanded. A click on the tab expands the workspace (`workspace.host.expand`),
  × closes it (`workspace.host.close`): collapsed and the tab gone, its inner tabs kept. Collapsing
  (⇧⎋, the stripe button, a code file) keeps it open and its tab in the strip; `open` is kept in
  `workspace.json`.
- **The Git workspace is a workspace too** (`git`, ADR-0187): its graph is the pinned tab `graph`,
  a diff opened from it while it is expanded is the inner tab `diff`, the interactive rebase editor
  `rebase` and the conflict editor `conflict`; the IDE view draws them, as a feature's view draws
  its tabs. Its button stays first on the right stripe.
- **The stripe button.** Each registered workspace has a button at the top of the right stripe,
  under the Git workspace's (which runs `workspace.host.toggle { id: "git" }`), with the icon the
  feature registered (`database` draws `glyph_database`): lit while expanded, "pressed" for screen
  readers, its tooltip the workspace's title. A click runs `workspace.host.toggle { id }`; the right
  tool windows start under the last workspace button. Find Action lists "Database Workspace"
  (`workspace.host.toggle/database`), which is how the modes that hide the stripes reach it.
- **Keys act on what is in front.** ⌘W closes the active inner tab (never a code tab hidden
  beneath); ⇧⎋ collapses while the workspace has the keyboard, and hides the tool window that took
  it otherwise; ⎋ never collapses. The Switcher and Recent Files list the consoles and the other
  inner tabs; picking one expands the workspace on it. On a tab the feature draws, editor commands
  and the window actions that need a text file are refused with a reason naming the tab, the status
  bar shows no caret or encoding, and `get_window_state` reports `editor: "workspace"` with
  `workspace: { id, expanded, activeTab }` and the view's popups. The conflict editor in front is
  refused the same way.
- **Kept across restarts.** `workspace.json` keeps each workspace's expanded state, the main group
  it returns to, its tabs in order with their kind data, the active one, each console's caret and
  first shown line and the panel sizes the feature reports (`WorkspaceView::panel_sizes`), read
  leniently. Restoring opens the main tabs, then the workspace's files that still exist, then its
  other tabs, then the expanded one's focus, and connects nowhere. A workspace not registered yet
  is kept as it was until its feature registers it. The tabs belong to the project: switching the
  window to another project (`file.project.open`) keeps them with the old project's layout and
  brings back the new project's, collapsed. The inner tab strip stays shown in Distraction Free,
  Zen and Presentation modes. Consoles are saved after a second of quiet even
  with autosave off.

### Commands

`workspace.host.list`, `.state`, `.expand`, `.collapse`, `.toggle` and `workspace.tab.open`,
`.select`, `.close`, `.move`, documented in [automation.md](automation.md#database). Nothing in the
host connects anywhere. A feature can pin tabs that are always there, first and not closable
(`Workspaces::pin`): the Database workspace's Overview is one.


## The Database workspace today

- **Registered by the module.** `zen_workbench::database::install` registers the workspace
  `database` (title "Database", icon `database`) in each window's session and pins its **Overview**
  tab (`kind: "overview"`, not closable). The window's view (`ide/database_view.rs`) draws a
  toolbar with **New Console** and the Overview: "No connections yet", a line on what comes next
  and **New Connection…** until connections exist, then the connections with their state and
  **Connect**/**Disconnect**; consoles are drawn by the editor as text tabs. A console needs a
  connection: with none, New Console opens New Connection… and `db.console.new` refuses with
  `noConnection` (the live check of 2026-10-27). Tabs of the same file name in a workspace are titled with what tells them apart
  (`Workspaces::label_tabs`): a console's connection name, `console.sql (Shop)`, else the folder.
  A bar too narrow for its buttons keeps the connection, schema and transaction controls and moves
  the less used actions into a ⋯ (More Actions) menu (`database_view::more_button`).
- **The toolbar is always whole** (ADR-0186). The connection and schema selectors, the lock, the
  one commit-mode control (its tooltip says what the mode does), Commit, Rollback and the
  pending-transaction counter (the open transaction's statements, 0 when none) are shown whatever
  tab is in front. With a console they are the console's; otherwise they show the connection of the
  tab in front (`database::tab_connection`: an object tab's `data.connection`), else the navigator's
  selection, read once a frame from memory (`database::toolbar_state`, never connecting), and act on
  its main context; the selectors only show it, with a tooltip saying to open a console to choose.
  Every button has a tooltip with its shortcut. New Console (⌃]) without a connection takes the
  navigator's selection, else the active tab's (`database::selected_connection`).
- **The statements that ran flash** for 0.8 s when a run ends (`db.console.state`'s `ran`, drawn as
  `TextExtras::flash`), not with Reduce motion.
- **Consoles.** `db.console.new { connection?, text? }` creates `console.sql`, `console_1.sql`, …
  under `<data folder>/consoles/<project key>/<connection>/` and opens it in the workspace, which
  expands. The project key is the project folder's name and the first 8 hex digits of the SHA-256
  of its path (two projects named alike never share consoles). **Until connections exist the
  connection folder is the placeholder `local`**; a connection's id takes its place when the
  connection registry lands. `db.console.list { connection?, limit? }` lists them, sized for agents.
  The consoles folder is one of the data folder's folders open to agents (`ui::ide::redact::AGENT_FOLDERS`,
  with the scratches, buffers and installed language servers), so agents read, complete and run
  consoles, and a console in front never makes another agent call refuse or post a warning.
- **Highlighting** is the SQL plug-in's (`plugins/sql`), as for any `.sql` file; without it the
  console shows the usual offer to install it.
- **Highlighting** is the SQL plug-in's (`plugins/sql`), as for any `.sql` file, and so are folding,
  the Structure and syntax problems, read in the console's connection's dialect (below, "The SQL
  plug-in"); without it the console shows the usual offer to install it, and everything else of
  this page works as before.
- **Nothing connects** by itself: the engine (below) connects only when asked.

## The engine and connections

- **`zen-db`** (L2) holds the model, the driver interface, the PostgreSQL, MySQL and MariaDB drivers
  and the lifecycle;
  its commands are `zen_workbench::database` (`sources.rs`, `connections.rs`), its view is the
  Overview. Commands are listed in [automation.md](automation.md#connections-and-connection-types).
- **The driver interface** (`zen_db::driver::Driver`, `DriverSession`) is by data model and
  capabilities (`zen_db::model::Capabilities`: models, query language, transactions, isolation
  levels, catalogues and schemas, node kinds, explain, server cancel, bind parameters, row
  identity, DDL preview, server paging, sessions, read-only sessions, URI schemes, a shared
  connection for embedded databases, the driver's settings). Results are one value type for every
  model (`zen_db::value::Value`: null, bool, integer, decimal as text, float, text, bytes, temporal
  as text with its type, interval, UUID, JSON, arrays, maps, documents, other). An in-memory
  key-value driver (`zen_db::memory`, feature `fake`) runs the same engine in tests: keys with
  prefixes as its tree, hashes as maps, MULTI/EXEC as transactions.
- **PostgreSQL 12–18** through `tokio-postgres` and `tokio-postgres-rustls` with the RustCrypto
  provider set on the connector (no process-wide provider, no `ring` or `aws-lc`): TLS modes as libpq
  names them, a CA file, a client certificate, PostgreSQL 17 direct TLS (with the `postgresql` ALPN
  the server requires), SCRAM and md5, keepalive, NOTICE messages kept with each result, cancel
  through CancelRequest. Statements run as a prepared statement and its portal (two round trips,
  columns known for empty results); behind a pooler as one unnamed statement.
- **MySQL 8.0+ and MariaDB 10.6+** through `mysql_async` without default
  features, with `minimal-rust` and `default-rustls-no-provider` (flate2's Rust backend, rustls with
  the RustCrypto provider installed as the process's provider when none is set, because
  `mysql_async` builds its TLS with the process provider; no `ring`, `aws-lc` or zlib), TCP only
  (`prefer_socket(false)`), TCP keepalive (the `keepAlive` setting), `caching_sha2_password` (its
  full authentication over plain TCP through the server's RSA key) and `mysql_native_password`. Two
  driver entries share the code: `mysql` (`mysql://`, dialect `sql:mysql`) and `mariadb`
  (`mariadb://`, `sql:mariadb`); the server's own flavour is read from `VERSION()` at connect. URIs
  accept `ssl-mode` with MySQL's values (`DISABLED`, `PREFERRED`, `REQUIRED`, `VERIFY_CA`,
  `VERIFY_IDENTITY`) besides `sslmode`.
  - Statements run on the **text protocol**, so values come as the server writes them: integers
    (an unsigned `BIGINT` past `i64` as a decimal), decimals as text, dates and times with zero dates,
    `YEAR` as a temporal, `BIT(1)` as a boolean and wider bits as integers, `JSON` (MySQL; MariaDB's
    `JSON` is `LONGTEXT`), enums and sets as text, binary strings, BLOBs, `GEOMETRY` and `VECTOR` as
    bytes. Column types are named from the protocol (`int unsigned`, `varchar`, `enum`).
  - **Row caps without reading everything**: a `SELECT` (the same test as PostgreSQL's paging) runs
    with `sql_select_limit` set to the rows wanted plus one, so the server stops there and the extra
    row says whether there are more; Fetch Next reads the statement again past the rows kept. The
    limit is set only when it changes and reset before anything else, the IDE's own queries
    included. Other statements are read whole (`CALL` results after the first are drained).
  - **Guards**: `max_execution_time` (MySQL, milliseconds, `SELECT` only) or `max_statement_time`
    (MariaDB, seconds) from the statement timeout; `lock_wait_timeout` and
    `innodb_lock_wait_timeout` for the Metadata context; MariaDB's `idle_transaction_timeout` for an
    idle transaction there; the session's name as the connection attribute `program_name` (sent only
    without TLS: `mysql_async` 0.37 leaves `CLIENT_CONNECT_ATTRS` out of its TLS request).
  - **Transactions** follow the server: manual commit is `SET autocommit = 0`, isolation and
    read-only are `SET SESSION TRANSACTION …`, and whether a transaction is open is read from the
    server's status flags after every statement, so DDL's implicit commit and a deadlock's rollback
    show at once. Warnings (`SHOW WARNINGS`, at most 50) become the statement's messages with their
    code; a syntax error's `near '…' at line N` becomes its position.
  - **Cancel** sends `KILL QUERY <id>` on a second connection made with the same settings. An
    operation dropped in the middle (a cancelled read of the structure) marks the session closed, so
    the next use reconnects instead of reading a stream left half way.
  - `verify-ca` behaves like `verify-full` for now: `mysql_async` 0.37 recognises a wrong host name
    by the text `NotValidForName`, which rustls 0.23 no longer writes, so a certificate for another
    name is refused (never accepted).
- **Values come back in binary** and are decoded by our own code (`postgres/decode.rs`): numerics
  exactly with their scale, dates and times as the server's ISO style writes them (with `BC` and
  `infinity`), intervals in the server's default style. `timestamptz` is written in UTC (`+00`),
  not in the session's time zone; types without a decoder come back as hex with their type name.
- **The lifecycle** (ADR-0164, decision 2): one runtime on the `zen-db` thread, operations with
  ids, cancellation tokens and deadlines, a waker for the window, Main + Metadata + one session per
  console, guards, ping after 30 s idle, reconnects with backoff restoring only tracked state,
  reads retried once and writes never, cancel within a second, idle close and idle transaction end
  per type, sleep detected by the wall clock jumping ahead of the monotonic clock.
- **Secrets** go through `zen_secrets::SecretStore` (below): the window's `Secrets` state is the
  process's Keychain store in the IDE and a `MemoryStore` in tests. The registry remembers that a
  password was saved (`passwordSaved` in the global file, in the person's overrides for a project
  connection), so `hasPassword` never reads the Keychain. `db.connection.connect` and
  `db.dataSource.test` take a `password` for one connection; without one, a saved password is read
  through the cached store on `zen-db`'s runtime (`StoredSecret`, `spawn_blocking`), never on the UI
  thread; with neither they refuse with `passwordNeeded`. A denied Keychain read fails the operation
  with code `secretDenied`. A read that has not answered after `Timing::secret_timeout` (30 s; a
  Keychain prompt nobody sees on a locked screen) fails it with `secretTimeout`; the blocked read
  goes on in the background, and once the person allows access the cache answers the next connect.
  While connecting, `ConnectionStatus.waitingFor` (also in `db.dataSource.list` and `get`) says what
  the connect waits for: `keychain`, `tunnel` or `network`; the Overview shows it, and its Cancel
  calls `db.connection.cancel`. Turning Save password off deletes the saved password.
- **Authentication by default**: `db.dataSource.add` (and the dialog for a new connection) gives a
  connection without a password `auth: none` when its server commonly needs
  none (`zen_db::registry::passwordless_by_default`: Redis and Valkey, SQLite, ClickHouse's
  `default` user), so it never refuses with `passwordNeeded`; an explicit `auth` stays as given.

## The connection dialog

- **One dialog** in `zen_ui::ide::connection_dialog`, drawn by `IdeView` and opened by the
  window action `view.dataSource.open { id?, driver?, url?, name?, host?, port?, database?, user?,
  folder?, type?, scope?, page? }` (Find Action "New Database Connection…"; `run_action` over MCP;
  the Overview's **New Connection…** and **Edit** and the workspace toolbar leave a
  `ConnectionDialogRequest` in the session, which the window opens the next frame). Find Action
  sends `db.dataSource.add`, `.update`, `.test` and `db.connection.setSecret` to it rather than
  running them without fields. It shows as the popup `connectionDialog`.
- **Fields from the driver.** `db.driver.list` answers each driver's `Capabilities`; the dialog shows
  Show all databases only for drivers with catalogues, schema filters and the default schema only
  with schemas, auto-commit only with transactions, the isolation levels the driver names, the
  password only when it asks for one, and every declared setting (`bool` as a check box, `choice` as
  a combo, others as a text field; `sslmode` is the SSL page's and `keepAlive` the General page's).
  Undeclared settings are the **driver properties** table. The in-memory key-value driver declares
  `mode` and `separator`, so its dialog is Redis-shaped.
- **URIs.** `zen_db::registry::parse_uri` splits `scheme://user:password@host:port/database?params`,
  also `rediss://` (TLS required), `mongodb+srv://` (no port) and several hosts
  (`host1:1,host2:2`, kept as the host). The scheme picks the driver by its `uriSchemes`
  (`driver_for_scheme`); `sslmode`, `sslnegotiation`, `sslrootcert`, `sslcert` and `sslkey` go to
  the SSL page, declared settings to their fields, the rest to the properties table. Pasting into
  Host works as well as into URL.
- **Test Connection** runs `db.dataSource.test` with the dialog's fields (and the edited
  connection's id, so a saved password is used when the field is empty) on `zen-db`'s runtime; the
  dialog polls `db.operation.state` and shows `test { serverVersion, connectMs, roundTripMs }` or
  the message with the likely cause (`DbError::cause`: DNS, refused, TLS, authentication, timeout).
  The person can cancel it (`db.connection.cancel { operation }`); closing the dialog cancels it.
  It saves nothing.
- **OK** runs `db.dataSource.add` (with the scope) or `db.dataSource.update` with every field; an
  empty password field keeps the saved password. The SSH fields (host or alias, user, port, jump
  hosts) open a tunnel (below).
- **Engine additions:** `DriverSession::server_version` (PostgreSQL `SELECT version()`),
  `OperationState::test`, `Bootstrap::isolation` (set on every context but Metadata), and the
  PostgreSQL setting `keepAlive` (seconds between TCP keepalive probes, 30 by default, 0 off).

## Detected databases and imports

- **Detection** (ADR-0165, decision 6) is `zen_db::detect` (reading `.env` files as dotenv does and
  the part of YAML Compose files use, with our own small reader: no YAML crate) and
  `zen_workbench::database::detect` (the scan and the `db.detect.*` commands). The scan runs on a
  thread of its own, started by the first look (`db.detect.list`, which the Overview calls every
  second while shown) and again after the window's file watcher reports a change to a `.env` or
  Compose file (`detect::files_changed`, called from `follow_disk`); it is cancelled when the project
  changes. Bounds: four folders deep, 20 000 entries, 64 files of at most 256 KB; `.git`, `.idea`,
  `.gradle`, the project's excluded folders and patterns, the declared excluded folders
  (`node_modules` beside `package.json`) and folders Git ignores are not entered, while a `.env`
  file Git ignores is read. Offers with the same settings are shown once (`.env` before its
  variants, examples last).
- **What an offer is**: a stable id from its file and key, the driver and product (MariaDB is the
  `mysql` driver, Valkey `redis`, ScyllaDB `cassandra`), host, port, database, user, whether the port
  is published, and the files its values came from. Its fingerprint (FNV-1a of the settings without
  the password) decides whether a dismissal or an added connection still matches: a change in the
  file offers it again. The password stays in memory (`zen_secrets::Secret`) until Add.
- **Add** opens the connection dialog with `detection` (the dialog fills every field, the password
  too, from `detect::offer`); OK runs `db.detect.accept`, which adds the connection through the same
  path as `db.dataSource.add` (`sources::add` with the offer's fields under the dialog's) and marks it
  with `origin`, or changes the connection made from the offer before (`sources::update`).
  Dismissals are kept per project in the IDE's data folder, never in the project.
- **Agents** (ADR-0143): an offer from a file hidden from agents, or a Compose service that read a
  hidden `.env` beside it, is answered with its id, kind, file and `redacted` only; `db.detect.accept`
  and `view.dataSource.open { detection }` refuse it.
- **Imports** (ADR-0165, decision 5) are `zen_db::import` (DBeaver JSON; JetBrains XML through
  `roxmltree`) and `zen_workbench::database::imports`. The ADR names `zen-jb-import` for the
  JetBrains reader; it is L5 like `zen-workbench`, which may not depend on it, so the reader lives in
  `zen-db` and, like `zen-jb-import`, never writes `.idea`. Credentials files are not opened.
- **The Import Connections dialog** (`ide/import_dialog.rs`, window action `view.dataSource.import`)
  takes DBeaver or a JetBrains project and a path (filled in with DBeaver's usual workspace or the
  open project), lists the entries with `preview`, and imports the ticked ones into the global or
  the project registry.

## Secrets

- **`zen-secrets`** (L0) keeps secrets for every IDE: `SecretStore` (`get`, `set`, `delete`),
  `Secret` (its `Debug` and `Display` print `***`; its text is zeroed when dropped), the macOS
  Keychain store (`keyring-core` 1 with `apple-native-keyring-store` 1.0.2 and its `keychain`
  feature, through `security-framework`; no C is compiled, no `unsafe` of ours) on macOS only, an
  unsupported store elsewhere (every call says so), and `MemoryStore` for tests (it counts accesses
  and can deny an item).
- **Ids.** `zen_secrets::database::id(project key?, connection id, kind)`: service
  `space.ifzenelse.zen.database`, account `<connection id>/<kind>` or
  `<project key>/<connection id>/<kind>`; `kind` is `password`, `ssh` or `token`. A connection id
  with a slash or a control character is refused, so ids never collide.
- **One access per secret per session.** `zen_secrets::system()` is the process's one store, a
  `CachedStore` over the Keychain: the first read of an item asks the Keychain, later ones answer
  from memory; storing and deleting through it update what it remembers.
- **Denied access** (a cancelled prompt, a locked keychain: `errSecUserCanceled`,
  `errSecAuthFailed`, `errSecInteractionNotAllowed`, no access to the keychain) is reported as
  `SecretError::Denied` and remembered: further reads answer Denied without asking again until
  `CachedStore::retry(id)`, which the person's explicit retry calls. Storing a secret always asks.
- **Removing a connection closes its tabs** (`database::removal`): its object, plan, sessions and
  key tabs (found by `data.connection`) and its consoles' result tabs close and their rows are
  freed; its open consoles stay, their tab data `{ "removed": <name> }` keeps them unbound (no
  fallback to the only connection left) until a connection is chosen, and `db.console.state` and
  the toolbar say which connection was removed. Staged data edits or structure changes in a tab
  that would close refuse the removal first with the usual question and `retry`.
  `db.dataSource.removal` counts what would close for the confirmation (`database-remove-question`).
- **Deleting a connection deletes its secrets:** `db.dataSource.remove` calls
  `zen_secrets::database::delete_all` on the window's store with the project key (or none) and the
  connection id; it tries every kind and reports the first failure.
- **Commands.** `db.connection.setSecret { connection, kind, value, scope? }` stores one through
  the window's `zen_workbench::database::Secrets` (the process's store, installed by the module;
  tests install a `MemoryStore`) and answers `{ connection, kind, scope }`. The connection must
  exist (`unknownConnection` otherwise; `scope`, when given, must be its scope); a password also
  turns on its Save password. No command reads a secret.
- **Tests** never touch the Keychain; one ignored test (`keychain::tests`) does, run by hand only,
  because it may show the system's prompt.

- **Storing and deleting run there too.** `db.connection.setSecret`, `db.dataSource.add`/`update`
  with a password or with Save password turned off, and `db.dataSource.remove` hand the Keychain
  work to `Engine::secret_work`: a blocking thread of `zen-db`'s runtime, in the order asked (a
  connect asked after a store reads what was stored), as an operation (`storeSecret`,
  `deleteSecrets`) the command answers at once (`operation`, `secretOperation`). The saved-password
  mark is set at once; a failed store takes it back and, like any failure, ends the operation with
  error kind `secret` and is posted as a notification (`Event::SecretFailed`).

## Running SQL and results

- **Statements** are split by our own line lexers (`zen_db::sql`, ADR-0164 decision 6), per dialect
  (PostgreSQL, MySQL, SQL Server, standard SQL, and one statement per line for Redis-like
  languages), incrementally: an edit lexes the lines it touched and the following ones until a line
  starts in the state it had (`zen_syntax::Lines`, ADR-0077), so a 10 MB script splits once in
  well under a second and an edit relexes a few lines. ⌘⏎ runs the selection or the statement under the
  caret with no chooser (blank lines end statements for it, setting `blankLines`: Always, Smart,
  Never); ⌥⌘⏎ runs the script; ⌃⌘⏎ keeps the result tabs; Execute from Caret runs to the end; the
  statement under the caret has a light band behind its lines.
- **A console's connection** is the one chosen in the toolbar's selector (kept with the tab in
  `workspace.json`), else the connection its folder is named after, else the only connection. The
  schema selector sets the console session's schema before its next statement. A run connects first
  when needed (with the stored password). A `.sql` file of the project runs only on the connection
  it is bound to (next section).
- **The results panel** below the console (resizable, hidden or maximised: `db.console.toggleResults`,
  `.maximizeResults`) has a **header line** with the statements run, rows changed and read, and
  execution and fetch time — no Statistics tab takes the focus — then a tab per result set
  (named after the table, `Result N` otherwise, numbered within each run: `orders`, `orders 2`,
  `Result 1`; a pinned tab keeps its name; at most 20 per run; pin, rename, close; a new run
  replaces the unpinned ones), **Output** (NOTICE, RAISE, WARNING) and the **Execution Log**. Every
  statement that ran shows its time and rows after it in the editor; a failure is underlined where
  the server points (its SQLSTATE position) with the message, and a script stops there and rolls back
  (setting `stopOnError`). Cancel (⌘F2, the toolbar, the header line) stops the statement on the
  server within a second.
- **The result store** ([ADR-0168](adr/0168-result-paging-without-held-locks.md)): typed columns
  with an overlay for staged edits, all results of a window within one budget (256 MiB); the first
  200 rows show at once and reading goes on to a cap of 10 000, through a portal committed at the
  end outside your transaction, kept open inside it; Fetch Next and Fetch All (asked first) read on,
  scrolling to the end reads the next page; "200+" counts on click. `timestamptz` is shown in the
  session's time zone.
- **The grid** (`ide/data_grid.rs`): virtualised both ways with a whole-number row offset (1 000 000
  rows scroll to the last), the visible cells formatted once and cached, column headers with a mark
  of their type and ▾, row numbers, resizing and fitting columns (double-click the edge), cell, row,
  column and rectangle selection with the keyboard, ⌘C and Copy As ([Copy and paste](#copy-and-paste)), dense rows, `[NULL]`
  in grey, numbers right-aligned, booleans as check boxes, long values cut with the whole in a
  tooltip; documents and maps show as `{key: value}`. Text and Record views (one row as a form with a
  filter of its columns); the last view is remembered. The bottom bar has Refresh, Fetch Next, Fetch
  All, the row count and "N rows – execution x s, fetch y s".

### Not yet

- Linux and Windows stores (ADR-0165, consequences).
- Of the "Database" keymap group, the workspace's own keys come first (ADR-0186: ⌥X, ⌘\\, ⌥P, ⌃⇧E, ⌃], ⌃⇧N,
  ⌃⇧F, ⌃/, ⌃4, ⌃8, ⌃9, ⌃0, ⌃T, ⌃⇧T, ⌃⇧D, ⌃⇧C, ⌃⇧V, ⌃⌥⇧T; an ⌥ key that runs an action does not type its
  character), and ⌘⏎, ⌥⌘⏎, ⌃⌘⏎, ⌥⌘E (Explain Plan), ⌘F2, ⌥⌘9, ⌥⌘0, ⌘S (Save Data Changes), ⌥⌘Z
  (Cancel Data Changes), ⌥⌘F (Focus Filter, `view.database.focusFilter`: a window action), ⇧⌘C and ⌥⇧⌘V (`view.database.copyAdvanced`,
  `view.database.pasteAdvanced`: window actions that open the dialogs; the commands are `db.result.copy` and
  `db.result.paste`) are bound (context Database), and ⇧⌘P shows or hides the value
  panel; the other keys come later. Next and previous statement stay
  unbound: ⌥↓ and ⌥↑ extend and shrink the selection in the editor.
- Several result sets of one call shown stacked: no compiled-in driver returns more than one result
  set per statement yet (the PostgreSQL driver runs one statement per call).
- The Refresh ▾ intervals in the bottom bar; column reorder
  and hide, and formatting cells off the UI thread, are left for the data editor.
- Huge cells cut in the SQL the IDE generates and fetched whole by key, so that BLOBs and long texts
  load lazily: object tabs read `SELECT *` and keep values whole; the grid cuts what it shows and the
  value panel renders at most 100 000 characters, off the window's thread. Staged edits work on the
  whole values kept, so they need no key-based reads yet.
- A console closed earlier and picked again in Recent Files opens in the main editor; only consoles
  open in a workspace go back to it.
- File pickers for the certificate fields, the import dialog's path and the navigator's custom view
  come with the features that use them; the import dialog takes a typed path.

## SSH tunnels and TLS

- **The system's OpenSSH** (ADR-0165; `zen_db::ssh`): a connection with an SSH host runs
  `ssh -N -L 127.0.0.1:<free port>:<database host>:<port> [-l user] [-p port] [-J jumps] -- <host or
  alias>` before its first session, so `~/.ssh/config` with its aliases, `IdentityFile`, the agent,
  `UseKeychain` and `ProxyJump` work as in a terminal. Options: `ExitOnForwardFailure`, a connect
  timeout, keepalives, and no multiplexing (`ControlMaster=no`, `ControlPath=none`, whatever
  `~/.ssh/config` says: with a shared master and `ControlPersist` ssh forks the forward into the
  background and exits, so the tunnel would look dead and outlive its connection). Host keys are
  ssh's own checks: nothing is accepted for the person. Values that start with `-` or hold spaces
  or quotes are refused, so no option can hide in a host name.
- **One tunnel per connection**, opened by the engine (`Engine::tunnel_address`) before Main, again
  when ssh died, and closed after the sessions on disconnect, idle close, sleep, removal and a
  failed connect; `db.dataSource.test` opens one of its own and closes it. Sessions connect to the
  loopback port while the database's host name still serves TLS (`ConnectSpec::via`): tokio-postgres
  `hostaddr`, MySQL's TLS host name override, and for ClickHouse and Redis a resolver that sends the
  real host name to the loopback port, so the certificate and `verify-full` check the real name and
  the tunnel never turns a check off. The private folder of the askpass socket is
  `/tmp/zen-ssh-<uid>`, mode 0700, owned by the IDE's own user (`geteuid`), never the temporary
  folder's owner (root on Linux).
- **Questions** (passwords, key passphrases, unknown host keys) come through **our askpass helper**:
  ssh runs with `SSH_ASKPASS` set to the IDE itself and `SSH_ASKPASS_REQUIRE=force`; the IDE started
  that way (`zen_ui::ide::read_command_line`, before anything else) sends the prompt over a Unix
  socket in that private folder and prints the answer for ssh. An answer is never written to a file
  or an argument. A saved SSH secret (Keychain `<connection>/ssh`) answers the first password or
  passphrase prompt; with jump hosts only a key's passphrase or the password prompt of the SSH host
  itself (`user@<host>'s password`), so a jump host never gets the server's password and its
  password is never saved as the server's; anything else becomes `Event::SshPrompt`, a dialog in the window (`sshPrompt`:
  a masked field with Save in Keychain, or Yes and No) and `db.ssh.prompts`/`db.ssh.answer` for
  agents; unanswered, it is refused after three minutes.
- **Errors** are what ssh said (`DbError` code `ssh`, the last lines of its standard error, such as
  "Could not resolve hostname", "Permission denied (publickey)", "Host key verification failed").
- **TLS** modes work as libpq's (`disable`, `prefer`, `require`, `verify-ca`, `verify-full`) with a CA
  file, a client certificate and its key (an unencrypted PEM key), tested against the image's
  self-signed certificate (`postgres_tunnel_tls.rs`).
- **Tests**: a fake `ssh` program (a script) checks the arguments, the tunnel's lifecycle, ssh's
  errors, a saved and an asked password, a jump host asked rather than sent the saved password, and a
  host key refused; ClickHouse and Redis through a tunnel send the real host name in their TLS hello; PostgreSQL through a tunnel whose
  forward the test serves; no real sshd is used (none is in the team's images).

## Messages for people and for agents

- A refusal people can read names controls, never command ids; the same refusal to an agent names
  the command to use (`database::for_caller` picks the `-agent` variant of the Fluent message when
  `zen_core::macros::by_agent`). Codes and data do not change. Covered: no connection for a new
  console, a console without a connection, no object tab, an unknown detection and an unbound
  script. Russian messages name the controls by their Russian titles («Новое подключение…»,
  the navigator of databases).

## Transactions

- **Modes per context** (`zen_db::engine::CommitMode`): Auto, Manual, Smart, and an isolation level,
  defaulted from the connection type and its overrides (Production manual, smart commit off) and set per context with `Engine::set_mode`, kept in the engine across
  disconnects; Metadata and pooler connections always auto-commit.
- **A new type applies at once** (`Engine::set_kind`): `db.dataSource.update` that
  changes a connection's type or its overrides, and `db.connectionType.update` for every connection
  of the type, hand the connected engine the type and overrides; the modes chosen per context give
  way to the type's, and each open session but Metadata is applied the new auto-commit and
  read-only setting (a reads-only transaction is rolled back first). A context whose transaction
  holds changes keeps manual commit until it ends (a warning names how many); nothing is committed
  by a type change. The confirmation before statements (`confirmExecute`) says whether the
  context commits each change at once or keeps it in the open transaction (`autoCommit` in its data).
- **Smart commit** (`Engine::smart_before`): before a statement in auto-commit with no transaction
  open, a statement that does not start with SELECT, SHOW, USE, SET or EXPLAIN (`zen_db::sql::
  smart_commit_switches`; for line languages the driver's `classify`) switches the context to
  manual; if it fails, the transaction is rolled back and the context goes back to auto; after
  Commit, Rollback or a typed `COMMIT` it goes back when the type's "return to auto-commit"
  (`autoCloseTransactions`) is on.
- **The transaction log** (`Engine::after_statement`): while a transaction is open, each statement is
  logged with whether it may change data (the driver's `classify`), whether it failed and the rows it
  changed; `BEGIN`, `COMMIT`, `SAVEPOINT` and the like are not counted. The log ends with the
  transaction (`TransactionState`).
- **The question.** Disconnect, Disconnect All, Reconnect, closing a console (a close guard of the
  workspace, `Workspaces::guard_close`), switching its connection and leaving manual commit refuse
  while a transaction has changes (`ErrorKind::OpenTransactions`); the refusal names the pending
  transactions and the command to run again with `transactions: commit | rollback`, which the
  window's dialog (`ide/transaction_dialog.rs`) does on Commit or Rollback. Reads only are rolled
  back without asking. Closing a console ends its session (`Engine::close_context`).
- **Idle transactions** are announced (`Event::TransactionIdle`) when idle as long as the type
  allows; the window shows a 10-second countdown (Keep Open, Roll Back Now) and the rollback follows
  unless `Engine::postpone` was called or the context was used; a notification says it ended.
- **The toolbar** (console): the mode menu (modes, isolation levels of the driver, Pending
  Transactions…), Commit and Rollback (disabled in auto-commit with nothing open), and the box with
  Auto, None or the statement count, green, orange from 5, red from 20, its tooltip listing the
  statements that changed data and the age, a click opening the log. A console tab with an open
  transaction has a dot (`WorkspaceView::tab_mark`). **Pending Transactions** lists every open
  transaction with Commit and Rollback.

## Structure and its snapshot

- **The model** is `zen_db::schema`, shared by every driver: catalogues, schemas (each with a level:
  `notRead`, `names` or `details`) and objects (tables, foreign tables, views, materialised views,
  functions, procedures, sequences, types) with their columns, constraints, foreign keys, indexes,
  triggers, policies, partitioning, sizes, row estimates, owners, comments and privileges; system and
  extension objects are flagged. It never holds data values or secrets. A driver whose tree is its
  own (key-value, document) keeps its `children` answers in the same snapshot instead
  (`Snapshot::nodes`), so Redis keys by separator or MongoDB collections come back offline too.
- **Reading** runs on the connection's Metadata context on the `zen-db` runtime, never on the UI
  thread, and can be cancelled (`OperationKind::Introspect`). PostgreSQL first reads the databases,
  the schemas and a **fingerprint per schema**: row counts and summed `xmin` over `pg_class`,
  `pg_attribute`, `pg_attrdef`, `pg_rewrite`, `pg_proc`, `pg_constraint`, `pg_index`, `pg_trigger`,
  `pg_policy`, `pg_type`, `pg_enum`, `pg_sequence`, `pg_inherits` and `pg_description` (DDL writes new
  row versions there; ANALYZE and VACUUM update in place, so they do not count). Only schemas whose
  fingerprint changed are read again, with **one query per object kind** for all of them, sent
  together on the one session (tokio-postgres pipelines them). A schema of more than 2000 objects is
  read by names; an object's details are read when it is expanded or asked for. Nothing is
  installed on the server. With the databases and schemas it also reads the current database's
  extensions, event triggers, the server's roles (with their memberships) and tablespaces (a size
  only where the role may read it), and with each table's details its rewrite rules (not a view's
  own `_RETURN`) and **dependencies** from `pg_depend`: the tables, views and functions a view's
  query uses, the views using a table (`usedBy`) and the sequences a table's columns own. These queries are optional: a PostgreSQL-compatible server that lacks a catalogue
  answers without them instead of failing the read. The snapshot format is version 5.
- **MySQL and MariaDB** keep the server's databases as the schemas of one catalogue named `def`
  (as `information_schema` names it), so consoles' schema selector and `db.schema.*` work as for
  PostgreSQL (their paths start with `def`); the navigator (`db.navigator.*` rows) skips a single
  catalogue when the driver's capabilities say `catalogs: false`, so the databases show right under
  the connection; system databases (`information_schema`, `mysql`, `performance_schema`, `sys`)
  are hidden unless system objects are shown. A database's **fingerprint** is a count and a sum of
  `CRC32` over the defining columns of `TABLES`, `COLUMNS`, `STATISTICS`, `KEY_COLUMN_USAGE`,
  `REFERENTIAL_CONSTRAINTS`, `CHECK_CONSTRAINTS`, `VIEWS`, `ROUTINES`, `TRIGGERS` and `EVENTS`, never
  `GROUP_CONCAT`, which `group_concat_max_len` (1024 by default) would cut. Every step sends its
  queries in one round trip as one multi-statement text and joins them in Rust (no joins of
  `information_schema` views on the server, whose collations differ). Tables, views, functions,
  procedures (with their parameters), events (a new object kind, folder `events`), triggers per
  table, columns (defaults as SQL, `AUTO_INCREMENT` as identity, generated columns, collations,
  comments), primary, unique, foreign and check keys and indexes (prefix lengths, functional
  indexes) are read; each object's exact `CREATE` statement comes from `SHOW CREATE`, fifty to a
  round trip, kept in `Object::definition` (`Routine::definition` for routines). `Driver::ddl`
  answers it offline, wrapping bodies in `DELIMITER $$`; a table without one is generated from its
  columns, keys and indexes. Names in generated SQL (`db.object.open`, `db.object.copyName`, the
  navigator's Generate SQL) are quoted with backticks for these drivers (`zen_db::schema::quote_for`).
- **The snapshot** of each connection is a postcard file with a header (magic `ZDBS`, format version,
  length, CRC-32), written to a temporary file, synced and renamed, at
  `<data folder>/caches/databases/<global or project key>/<connection id>.snapshot`. A damaged file
  or one of another version is ignored and read again from the server. It is read on a thread of its
  own when the navigator or a command first needs the connection, before connecting, so the tree
  and completion have the structure at once and offline.
- **Refreshes** happen in the background when a connection connects, after a statement that changes
  the structure ran anywhere on it (`Driver::changes_structure`: CREATE, ALTER, DROP, COMMENT,
  GRANT, REVOKE, TRUNCATE…; the engine raises `Event::StructureChanged`), and on Refresh. Nothing
  connects for it: a disconnected connection is not read.
- **DDL** is generated offline from the snapshot by the driver (`Driver::ddl`): tables (columns with
  identity, generated, default, collation; keys and checks inline; partitioning), views,
  materialised views, functions and procedures (the server's own `pg_get_functiondef`), sequences
  with their owner, enum, composite, domain and range types, indexes, constraints and triggers. The
  toggles, all off: nested objects (indexes, triggers, policies), permissions (GRANTs from the
  privileges, the owner's own left out), separate foreign keys, separate constraint indexes,
  comments, partitions.
- **Not yet:** the other databases of a server are listed but read only through a connection of
  their own, which the engine does not open yet; foreign tables' server and options are not read.

## Completion and navigation in SQL

- **Where.** Consoles and the `.sql` files open in the Database workspace, on their connection;
  `zen_db::assist` (pure, compiled for WebAssembly too) and `zen_workbench::database::assist`.
- **Decision: our own resolver, not squawk.** ADR-0164 allows `squawk-parser` for PostgreSQL's
  syntax tree. Completion reads an unfinished statement at the caret, where any parser's recovery
  shape is arbitrary, and the same contexts are needed for MySQL, MariaDB, SQLite and ClickHouse.
  One tolerant resolver over the dialect's tokens (the filter box's scanner) serves all five, adds no
  dependency tree (`rowan` and its crates), and its unit tests cover the tricky cases (unclosed
  parentheses and quotes, comments inside, CTEs with and without column lists, `*` through
  subqueries, correlated subqueries, quoted mixed-case names, MySQL backticks). squawk stays the
  choice for the SQL preview of structure changes, if that needs a real tree.
- **What it offers**, from the snapshot without a round trip: keywords per dialect (statement
  starters at the start), schemas, tables and views (the console's schema first, other schemas
  qualified), CTEs, columns of the tables in scope (shared names qualified), functions of the
  snapshot and built-in ones per dialect (`name()` with the caret inside), table aliases, and joins
  from foreign keys both ways after `JOIN` (`customers c ON c.id = o.customer_id`, aliases from
  initials, numbered when taken) and the condition after `ON`. Nothing inside literals, comments or
  numbers. Keywords take the case typed; nothing changes case while typing.
- **Missing details.** A table of a schema read by names only has no columns:
  completion asks for its details in the background when the connection is connected and shows the
  list as pending; the list fills in when the snapshot changes. A snapshot still being read from disk
  makes the list pending the same way.
- **Navigation and description.** `navigate.declaration.goTo` (⌘B, ⌘-click), F4 (`db.sql.navigate`,
  Database keymap) open the object tab (a column on its table's Columns); F1 and the ⌘ peek describe
  it (kind, comment, columns with types, not null and primary key).
- **Warnings.** Unknown tables and columns, only where certain: tables in schemas read in full (not
  `pg_*`, `sqlite_*`, `dual`), never DDL targets or tables the script creates; columns of tables read
  in full, unqualified ones only outside parentheses, never aliases, parameters, casts, typed
  literals or keywords. Up to 256 KB of text, cached per text version and structure revision.
- **Redis** consoles complete command names (with their arguments) and, in key arguments, the keys
  the tree has read. **ClickHouse** completion reads its tree (databases, tables, columns read) as a
  relational view.
- **Not yet:** `*` expansion into a column list; Redis key completion by a bounded `SCAN` (only keys
  already in the tree).

## SQL in code strings

- **Where.** Language injection is platform code (ADR-0183): `zen_syntax::inject` holds the
  mechanism (string literals from a file's highlighting, an `Injector` per language, the fragments
  kept by `Highlighting` in step with edits, their spans laid over the literal's), `zen_db::inject`
  the SQL rules (`SqlStrings`), `zen_workbench::database::injected` the fragments of an open file,
  its connection and the `db.injection.*` commands, and `assist` the completion, navigation and
  warnings in them. The window gives `Highlighters::set_injectors` the SQL injector for code files.
- **Which literals.** Go, Rust, Python, JavaScript, TypeScript, Java and Kotlin files
  (`zen_db::inject::hosts`). A literal holds SQL when a `language=<name>` comment on its line or the
  line before names an SQL dialect (any other name keeps it out), when a `sql` tag (or `sql<T>`)
  stands before a template literal, or when its text reads as a statement
  (`zen_db::inject::looks_like_sql`): the first word upper or lower case (never "Select a file"),
  then the words it needs (`SELECT x` in capitals, `select … from`, `INSERT INTO`, `UPDATE … SET`,
  `DELETE FROM`, `WITH … AS … SELECT`, `CREATE`/`ALTER`/`DROP` and an object word, `GRANT … ON … TO`,
  `EXPLAIN`). Bare `"DELETE"` or `"UPDATE"` (HTTP methods, enum values) are not SQL.
- **Literals.** Pieces of one literal on several lines (an incremental highlighter's per-line
  spans) are joined; prefixes and quotes are taken off (`r#"…"#`, `b"…"`, `f"…"`, `'''…'''`,
  triple double quotes, backticks); escapes are read (`\"`, `\\`, `\n`, `\t`) except in raw
  literals, and `Code` maps every byte of the SQL back to the file, so completion's `from`, warnings
  and ⌘B land on the right characters. Completion writes escapes back into an escaping literal.
  Literals over 64 KB are skipped.
- **Costs nothing without SQL, never blocks typing.** A cheap scan (`Injector::may_hold`) skips texts
  without a statement word; an edit looks again only at the lines its highlighter lexed again, one
  line around them and the literals they touch, and reuses the highlighting of a literal whose text
  is unchanged. Highlighting a literal uses the SQL plug-in's highlighter when installed, else
  `zen_db::inject::highlight` (keywords of every dialect, functions, literals, numbers, comments,
  `$1`/`?`/`:name`). Theme colours only: the injected spans are ordinary `HighlightKind`s; the text
  between them is plain, the quotes keep the string's colour. Only the editor draws the injected
  spans; bracket matching, spelling and search keep reading the literal as a string.
- **The connection.** The file's own binding (`db.injection.bind`, kept with the scripts' bindings
  in `<data folder>/databases/scripts/<project key>.json`) or the project's default
  (`db.injection.setDefault`, the `default` of the same file); only connections that exist count.
  Completion, ⌘B, F1 and the warnings then work on its snapshot as in a console, in its dialect;
  the snapshot is read from disk when the file's problems are asked for, never connecting. Without a
  connection completion offers keywords and `db.sql.*` refuse with `noBinding`.
- **Running it** (C11): `db.injection.run` takes the literal around the caret or
  `offset` (its whole literal, prefix and quotes included, so the caret on a quote counts), splits
  its `Code` in the connection's dialect and answers the statement under the caret (`statement`),
  the selection inside the literal, every statement (`script`) or those from the caret; each
  statement's `start`/`end` are characters of the file (through `Code::to_text`), so times, failures
  and the server's error position are marked on the code. A time hint goes at the end of the line
  where the statement ends (after the closing quote and `);`), never inside the literal; in a
  console it goes after the statement's `;` (`consoles::hint_place`). The Execution Log gives a
  time only to statements that ran (done, failed, or cancelled after starting). While a layout
  change has the editor partly out of view (the workspace opening), it does not scroll to the
  caret, so the code keeps its scroll. The run itself is the console's
  (`consoles::start`): a code file is a console keyed by its path — `consoles::connection_of` and
  `schema_of` answer `injected::binding`, `pick_bound` asks `injected::statements`, and
  `console_path` accepts code files, so `db.console.state`, `.statement`, `.execute`, `.explain`
  and the results panel work for it. Bind parameters (`variables::prepare`, the Bind Parameters
  dialog through `parametersNeeded` with a retry of `db.injection.run`), the safety checks of the
  connection's type (ADR-0172), the history and the file's own session (closed with its tab) all
  come from there. The file opens as a tab of the Database workspace (as a `.sql` script does),
  where the results show below it.
- **In the editor.** ⌘⏎ (the editor's `db.script.run` key) runs the literal at the caret of a code
  file when it holds SQL and does nothing elsewhere; each literal with SQL has a run icon on its
  first line (from `db.injection.list`, after the file's own run targets); ⌥⏎ lists **Run SQL** and
  **Choose SQL Connection…** (or **SQL Connection: <name>…**) after the language server's actions
  (`injected::intentions`, ids `RUN_ACTION`, `CHOOSE_ACTION`). We chose a chooser at the caret
  (`sql_connection_popup.rs`) over a bar above every code file: a bar would sit over Go and Rust
  files that rarely run SQL. The chooser opens for an unbound run (it then goes on) or from ⌥⏎; it lists the
  connections, the current one checked, and "For every file of the project", which sets the
  project's default and makes the file follow it.
- **Not yet:** a query concatenated from several literals; `${…}` in templates read as text; a
  background for injected fragments; the type colour of each connection in the chooser.

## Find Usages of tables and columns

- **Where.** `zen_db::assist::usages` finds the places of one statement that name a target
  (`Target::Object` or `Target::Column`) by resolving every candidate word as ⌘B does
  (`resolve_token`: aliases, CTEs, subqueries, schemas, quoted names), and marks each `Read` or
  `Write` (the first table of an `INSERT`, `UPDATE`, `DELETE`, `MERGE`, `TRUNCATE`; a column in an
  insert's column list or on the left of `SET`; none for DDL). `zen_db::assist::mentions` finds a
  name as a word in text no statement reading can resolve (routine bodies, dollar-quoted bodies,
  trigger definitions), outside comments and strings, with a qualifier check.
  `zen_workbench::database::usages` holds `db.usages.find` and `db.usages.rename`.
- **The search.** A tab of the Find tool window (`find_results::provided`, `ResultsKind::Database`)
  runs a whole-word text search for the name over `*.sql` and the code files SQL strings are read in
  (`zen_db::inject::host_extensions`) plus the connection's consoles outside the project, in the
  background as a cancellable task; the `UsageFinder` keeps only real usages: a `.sql` file or
  console whole, a code file's SQL fragments (`SqlStrings` over its highlighting), each split into
  statements and resolved against the snapshot in the file's schema. A file counts when its binding
  (script binding, injection binding, else the project's default for code) is this connection or
  none. Then the snapshot's views (their definition), functions and procedures (their body, by
  `mentions`; for a column only when the body names its table), triggers on the table and foreign
  keys to it are listed under the connection's name, each opening its object tab (`command` on the
  result node, `db.object.open`).
- **Rename** (`db.usages.rename`) opens the same tab with `newName`: `search.results.replace`
  replaces only usages in project files; consoles and the database's objects stay listed. The
  Properties page's **Rename…** and **Edit Column** forms open it after staging the change, when
  **Find usages in the project to rename** is ticked. The database changes only through
  `db.structure.execute`.
- **⌥F7** in a console, SQL file or SQL string goes to `navigate.usages.find`, which hands a name
  the snapshot resolves to `database::usages::find_at`; the navigator's ⌥F7 and context menu and the
  object tab's **Find Usages** button call `db.usages.find`. Never connects: without a snapshot it
  refuses with `noStructure`.
- **Not yet:** usages in other connections' snapshots; dynamic SQL inside routine bodies resolved
  as statements (bodies are read by name); ⇧F6 straight from the navigator.
## The SQL plug-in

- **What.** The SQL plug-in (`plugins/sql`, 0.2.0, ABI v1, [ADR-0182](adr/0182-sql-plugin-abi-v1-and-dialects.md))
  gives any `.sql` file, with or without a connection and with or without database support:
  folding (statements, brackets, `BEGIN`/`CASE … END`, comments, function bodies, `-- region`),
  the Structure (each statement; created objects by name with a table's columns; `WITH` queries'
  CTEs), keyword completion (the dialect's keywords, types and built-in functions, what follows the
  previous keyword first) and syntax problems (unclosed strings, names, comments and dollar quotes,
  brackets, `CASE` without `END`, a comma before a clause). Through the editor's commands:
  `editor.fold.*`, `editor.structure.get`, `editor.completion.*`, `problems.file.list`.
- **Dialect from the bound connection.** In a console or a bound project file, the host tells the
  plug-in the connection's dialect (`zen_dialect`: `postgres`, `mysql`, `mariadb`, `sqlite`,
  `clickhouse`) before every call; MySQL's `#` comments and backslash escapes, PostgreSQL's dollar
  quotes and nested comments, SQLite's `[names]` and ClickHouse's trailing commas are read as that
  server reads them. Without a binding, a problem is reported only where PostgreSQL's and MySQL's
  readings agree.
- **No double completion.** In a bound file the connection's completion (above) is the list, and it
  already has the dialect's keywords; the plug-in's items come only in other `.sql` files.
- **Bundled** in the IDEs with database support on by default, GoGarden, RustRonin, WebWave, SqlScroll
  and DevopsDaruma (`PRODUCT_PLUGINS`); offered from the catalog elsewhere.
- **Not yet:** highlighting in the bound dialect (it colours every dialect at once); a dialect from
  the file's extension (`.pgsql`, `.mysql`); a binding changed without an edit refolds at the next
  edit.

## Formatting SQL

- **Where.** `zen_db::format` (pure, compiled for WebAssembly too) and
  `zen_workbench::sql_format`; Reformat Code (`editor.code.reformat`) sends every SQL file and
  console there instead of a language server, unless the project declares a formatter of its own
  for the file, in every IDE, with or without the database module; `db.sql.format`, which formats
  text for agents, comes with the module. Settings → Editor → Code Style → SQL (`sqlFormat` in the IDE settings,
  `editor.sql.format` in the session) holds its options next to the SQL indentation.
- **Decision: our own formatter, not sqruff or sqlformat.** sqruff (a Rust port of SQLFluff, MIT)
  parses by a full grammar per dialect and brings a large dependency tree for a feature whose
  first rule is to change nothing but white space; the `sqlformat` crate re-tokenises by its own
  rules and joins tokens it does not know. Ours reads tokens with the same dialect scanner as
  completion and the filter box (`filter::scan`: quotes, `E''`, dollar quotes, backticks, brackets,
  `#` and nested comments), splits statements with the Execute Script splitter, and lays out only
  the white space between tokens: clauses on their lines, list items and `AND`/`OR` indented,
  subqueries and `CREATE TABLE` columns as indented blocks, calls and `IN` lists inline.
- **Never surprising.** Each statement's result is scanned again and must give the same tokens
  (words compared without case) or the statement is kept as it was; statements with an unclosed
  quote or comment or unmatched brackets are kept and counted (`kept`); client commands (psql,
  `DELIMITER`, `GO`) are skipped; procedural statements (`BEGIN`, `DECLARE`) and statements other
  than queries, DML and `CREATE TABLE`/`VIEW` only get keyword case. White space is never removed
  between a name and `(` (MySQL reads `COUNT (` otherwise) and never added where tokens touched,
  except after a comma, inside the brackets of a subquery or a column list, and before a clause
  keyword that follows a bracket or a comma. A line comment
  always ends its line. Keyword case touches only words reserved in PostgreSQL and MySQL alike
  (plus `KEY` after `PRIMARY`/`FOREIGN` and statement starters such as `BEGIN`), never after or
  before `.`, never a function name before `(`, never a variable (`:name`, `@x`).
- **Off the UI thread.** Reformat Code formats in a thread of its own; the command waits up to
  `waitMs` and answers `pending`, a changed document or another request cancels the run between
  statements, and the edits are applied as one undo step for the version they were made for.
- **Tested** by a corpus of PostgreSQL, MySQL, SQL Server and standard scripts under every option
  combination and three indentations: formatting twice gives the same text, the tokens are the
  same, with As is every token keeps its case, and the statement count does not change.
- **Not yet:** a line width that wraps long expressions; aligning `AS` aliases; formatting inside
  procedural bodies.

## Scripts: `.sql` files of the project

- **Bound, never guessed.** A `.sql` file of the project (`zen_workbench::database::scripts`) runs
  only on the connection and schema it is bound to; no folder name or "only connection" stands in
  for a binding. A run of an unbound file is refused with code `noBinding` and the connections to
  choose; the window opens the bar's connection list and runs once one is chosen.
- **Where the binding lives.** Per project in the IDE's data folder,
  `<data folder>/databases/scripts/<project key>.json`, keyed by the path in the project. Not in the
  file and not in `.zen/`: a binding may name a
  global connection, which is personal (ADR-0165), and it is a person's working choice, like
  `workspace.json`. A console's binding stays with its tab. A file renamed or moved loses its
  binding and asks again.
- **In the code editor.** A bar above a `.sql` file of the project (`ide/sql_script_bar.rs`, not in
  the workspace's own group) has Execute, Execute Script, the connection selector (with the
  connection type's colour) and the schema selector; ⌘⏎ and ⌥⌘⏎ (keymap actions `db.script.run`,
  `db.script.run/script`, editor context, only in `.sql` files) and a run icon per statement in the
  gutter (`db.script.targets`, like the HTTP Client's) run it. A run opens the file as a tab of the
  Database workspace with the console's results panel below it and expands the workspace.
- **Assistance and dialect.** Splitting, completion, navigation and the warnings take the bound
  connection's dialect and snapshot, as in a console (`assist::bound` accepts bound scripts).
- **Scripts per connection.** The navigator's SQL Editor menu (on a connection or an object) has
  Open Console, **New Script…** (a save dialog that starts in the project; `db.script.new` refuses a
  folder outside the project or a hidden one such as `.git`, and never replaces a file) and
  **Scripts**: the connection's bound files and consoles (`db.script.list`), each opening as a tab of
  the workspace.
- **Not yet:** the binding following a file renamed or moved in the IDE; scripts shown in the
  navigator's tree.

## One script on several connections

- **Command** (`zen_workbench::database::several`): `db.script.runOnConnections` takes a console or
  a `.sql` file, the connections in the order they run (at most 20, duplicates dropped) and
  `stopOnError` (true by default). It reuses the console runner: `consoles::pick` takes the same
  statements as Execute Script (or `statement`, `fromCaret`), split in each connection's dialect;
  `consoles::check` runs `safety::guard` per connection with the history source `connections`;
  `consoles::begin` starts each query in the console's own context on that connection, without
  changing the console's connection. `db.script.connectionsState` reads the run (`waitMs`).
- **Safety before anything runs** (ADR-0172 applied per connection): every connection is checked
  first. Any hard refusal (`readOnly`, `agentReadOnly` with its `handOff`, `agentNotAllowed`)
  refuses the whole run with that code and every refusing connection; otherwise any `confirmExecute`
  refuses it once with `targets` naming every connection, its type and the statements that ask
  there, and a token hashed over the connections in order and each one's own token (so a reordered
  list, another connection or an edited statement asks again). On the confirmed run each asking
  connection is checked again with its own token, so the history marks it confirmed. Asking up front
  for all connections, not one by one while the run is half done, follows ADR-0172's
  "Alternatives considered".
- **In turn, cancellable.** One connection runs at a time: `several::step` (in `consoles::step`, each
  frame, and in `db.script.connectionsState`'s wait) absorbs a finished query's results, then starts
  the next connection, which connects only then. A failure with `stopOnError` marks the rest
  `skipped`; `db.operation.cancel` cancels the running query and marks the rest `cancelled`; a
  console busy with another run waits. Parallel runs were left out: the connections run in turn, and
  one console's results panel follows one query at a time.
- **Grouped per connection.** The first connection replaces the unpinned result tabs (unless
  `newTab`); later ones add theirs, titled `<connection> · <title>` with `connection` set. The panel
  shows a **Connections** tab (`PanelTab::Connections`, kept while the run's tabs arrive): each
  connection's status, statements, rows, time and error; a click shows its first result.
  `db.console.state` answers the run as `connections`.
- **Window**: `several_dialog.rs` (a transaction dialog, `severalConnections`): connections ticked in
  order with their types, Stop at the first error, then the confirmation listing every connection or
  the refusals, Run Anyway after "I understand" for risks, Enter consumed, window shortcuts off. The
  button sits after Execute Script in the console toolbar and in the bar above a `.sql` file.
- **Not yet:** running connections in parallel; comparing two connections' results row by row (a later
  compare feature); a saved set of connections to run on.

## The Database Navigator

- **Where.** The left panel of the Database workspace (`ide/database_navigator.rs`), resizable by
  its right edge and hidden to a strip from its Options menu; its width is one of the panel sizes
  the workspace keeps.
- **The tree.** Folders of connections (a connection's `folder`), connections (the database icon, a
  dot when connected, a red badge after a failed connect, a lock when read-only, `host:port` in grey
  when Show Host is on, the row tinted with the connection type's theme colour), then the driver's
  tree: databases, schemas, folders per object kind (Tables, Foreign Tables, Views, Materialized
  Views, Functions, Procedures, Sequences, Types) and objects; a table has Columns (`name (type)`),
  Constraints, Foreign Keys, Indexes, Triggers, References (other tables' foreign keys to it),
  Policies and Partitions. On PostgreSQL a schema always lists Tables, Foreign
  Tables, Views, Materialized Views, **Indexes** (every index of its tables, `[db, schema, indexes,
  name]`), Functions, Sequences, Types when there are any, and **Aggregate Functions** (aggregates
  leave Functions; `[db, schema, aggregates, name(arguments)]`, found as functions); a table always
  has Policies and Partitions; and the current database lists, after its schemas, **Extensions**,
  **Event Triggers**, **Tablespaces** and **Roles** (folders `#extensions`, `#eventTriggers`,
  `#tablespaces`, `#roles`, items `[db, #roles, name]`; the server's own `pg_*` roles are system
  objects). Each of these opens in an object tab with only a Properties page, whose one section,
  Settings, lists its properties. Tables show their size as text (`72K`, `1.5M`) in a column of its own at
  the right edge, over a bar against the largest sibling, with the size and row estimate as its
  tooltip (Show Statistics); the column is left out where it would leave the name less than 64
  points. Every row is painted inside its own rectangle: long labels end with "…" and nothing draws
  outside the panel. Icons are our own glyphs.
- **Lazy.** Children come from the snapshot at once; otherwise they are read in the background and a
  "Loading…" row stands for them. A failed read collapses its node and puts the error on it and
  under the tree. Expanding a disconnected connection, a double click or ↩ on it connects it (the
  person's action; a missing password opens the connection dialog). A selection made elsewhere
  (`db.navigator.select`, Search Everywhere, Link with Editor) scrolls the tree to the row.
- **Toolbar**: New Connection ▾ (a driver, or the dialog), Connect, Reconnect, Disconnect, Refresh,
  Collapse All, Link with Editor (selects the connection of the console in front) and Options: the
  view preset (Simple: tables, views and columns; Advanced, the default; Custom, for now as
  Advanced), Show System Objects (off), Show Host (on), Show Descriptions (off), Show Statistics
  (on), Show Connected Only (off), Hide.
- **Filter.** The box under the toolbar keeps rows whose names contain the text, among what was
  read, with their ancestors expanded; typing in the tree starts it, ⎋ clears it.
- **Keys** while the tree has the keyboard: arrows, Home, End; → expands, ← collapses or goes to the
  parent; ↩, F4, ⌘↓ and a double click open an object in its object editor on Properties,
  with Data the next page (F4 also a schema); F4 on a connection edits it; ⌘N opens
  the connection dialog; F5 and ⌘R refresh, keeping every expanded row expanded; F2 renames and ⌦
  (or ⌘⌫) deletes the selected connection or object.
- **Context menus**: a connection: SQL Editor ▸ Open Console, Edit Connection…, Duplicate
  Connection, Connect, Reconnect, Disconnect, Copy Name, Rename… F2, Delete… ⌦, Refresh; an object:
  View Data, Open Object, SQL Editor ▸ Open Console, Create ▸ (Table, View, Sequence, Function,
  Schema; Column and Index under a table), Generate SQL ▸ DDL, SELECT, INSERT (into a new console),
  Tools ▸ Truncate… (tables), Copy Name, Copy Qualified Name, Rename… F2, Delete… ⌦, Refresh.
- **Changing objects from the tree**. Delete, Rename and Truncate run at once, not
  staged on a tab: `db.object.drop`, `db.object.rename` and `db.object.truncate` write
  the statement with the structure-editing script writer (`zen_db::alter`, the dialect's own SQL;
  `DELETE FROM` for a SQLite truncate) and run it through the same path as `db.structure.execute`:
  the person's action in the Main context, production safety (ADR-0172), a SQLite rebuild's
  foreign-key handling, and afterwards only the changed schema read again. The run is kept under the
  key `navigator:<connection>` of the window's structure changes; when it ends, object tabs of what
  was dropped close and those of a renamed object open again under the new name. A drop or truncate
  always asks first: its `confirmExecute` refusal carries `loss`, the words naming what is lost
  (rows and size of a table from the snapshot, its columns, indexes and triggers, the foreign keys
  pointing at it, a schema's objects), and `cascade`, whether the server can cascade; the
  navigator's Delete dialog shows them with the statement and a Cascade box, and runs again with the
  token. A rename confirms only where the connection type asks. Create ▸ (`db.navigator.create`)
  opens the schema's or table's tab on Properties and the existing structure form for the new object
  (`CreateRequest` in the session, taken by the window), so a new object goes through the staged
  script and its preview. Duplicate Connection (`db.dataSource.duplicate`) copies the registry
  entry, the person's overrides and the saved secrets (in the background) under "Name (copy)".
- **Kept per project**: expansion, selection, collapsed folders and the options in
  `<data folder>/databases/<project key>/navigator.json`; the filter is not kept.
- **Not yet:** Tools ▸ other than Truncate, Recent Scripts, the driver icons per database (one database glyph for all), the Custom view's own
  choices, and the keymap's Database group (the navigator's keys work while it has the keyboard).

## SQLite

- **The driver** is `zen_db::sqlite` (ADR-0164, decision 4.2): the system `sqlite3` program, no C in
  our build. One long-lived process per connection (`sharedConnection`: Main, Metadata and every
  console use it), started with `-safe -batch -init /dev/null [-readonly] file:<path>?mode=rw|ro` (or
  `:memory:`) through `tokio::process`, never a shell, in a process group of its own and killed when
  the session goes. It is set to `.mode quote`, `.headers on`, `.explain off`, `.bail off` and
  `.timeout 5000`. `-safe` keeps everything inside the database, even SQL a view or trigger of the
  file holds: no `.shell`, `.system` or other dot-command that opens a file, no `ATTACH` (and so no
  `VACUUM INTO`), no `readfile()`, `writefile()`, `edit()`, `load_extension()` or
  `fts3_tokenizer()`; the program ends on such SQL, and the session opens again on next use.
- **Framing** (`sqlite/process.rs`): each statement is sent as `<sql>\n;\n` followed by
  `.print <marker>`, `SELECT changes(), total_changes();`, `.print <marker>` and the unknown
  dot-command `.<marker>` (the marker is per session and per statement). Standard output is read to
  the second `.print` (the rows, then the counts), standard error to the line naming the unknown
  command (the statement's errors), both at once while the input is written, so neither pipe fills.
  A process that ends before its markers is a lost session (or a cancelled one).
- **Only plain SQL goes in** (`sqlite/statement.rs`): our own scanner refuses an unterminated
  quote, identifier or comment, a `CREATE TRIGGER` without `END`, a second statement and anything
  starting with `.` (dot-commands such as `.shell`), any later line `sqlite3` would end a statement
  on (only `/` or `GO`, then spaces or a comment) or run as a dot-command (a `.` in its first
  column), and `ATTACH` or `VACUUM INTO`, so nothing after a statement can be read as part of it or
  as a command of the program, and no file outside the database is opened.
- **Values** (`sqlite/quote.rs`) are read from the quote mode's literals: `NULL`, integers, reals
  (`Inf`), `'text'`, `X'…'` blobs as bytes, and text with line breaks as `unistr('…')` (recent
  versions) or `replace('…','\n',char(10))` (older ones). A result column's type is the storage class of
  its values (`INTEGER`, `REAL`, `TEXT`, `BLOB`; `ANY` when mixed): the program does not report
  declared types of results. It names the columns only above a first row, so a query (`SELECT`,
  `WITH`, `VALUES`) that returns none has its columns read from a temporary view of it
  (`pragma_table_info`, with `query_only` lifted for that view in a read-only session), without
  running it again; an empty table shows its columns and no rows. `affected` comes from `changes()`
  versions) or `replace('…','\n',char(10))` (older ones). A result column traced to a table's column
  (a plain `SELECT` of one table) takes the type the table declares (`NUMERIC`, `DATE`, `JSON`,
  `BOOLEAN`…), read with `pragma_table_info` before the statement; any other column's type is the
  storage class of its values (`INTEGER`, `REAL`, `TEXT`, `BLOB`; `ANY` when mixed), since the
  program does not report declared types of results, and an empty result has no columns. `affected` comes from `changes()`
  for `INSERT`, `UPDATE`, `DELETE`, `REPLACE` (and `WITH …` when rows changed). Errors keep
  SQLite's message; the context and caret it prints give the one-based position in the statement.
- **Structure** (`sqlite/introspect.rs`): `pragma_database_list` gives the schemas (`main`, `temp`
  when it holds objects) under one catalogue named after the file; each
  schema's fingerprint is its `schema_version`. A changed schema is read from `sqlite_master`,
  `pragma_table_xinfo` (declared types, NOT NULL, defaults, primary key, generated columns),
  `pragma_foreign_key_list` and `pragma_index_list`/`pragma_index_xinfo` (unique constraints from
  their indexes); triggers from `sqlite_master`. The DDL is the `CREATE` statement SQLite keeps,
  with the indexes and triggers when nested. The navigator shows no Functions or Sequences folders
  for SQLite.
- **Transactions**: auto-commit, manual and smart commit as for every driver; manual commit starts
  with `BEGIN DEFERRED`, `IMMEDIATE` or `EXCLUSIVE`, offered as the driver's isolation levels.
  Session read-only is `PRAGMA query_only`.
- **Cancel** sends SIGINT to the program: it interrupts the statement and, reading a pipe, ends; the
  session is then closed and opens again on next use (`Reconnected`), so an open transaction is
  rolled back. An interrupt that reaches the program after its statement finished (a cancel that
  comes late, as when a table is read again at once) keeps that statement's rows but ends the
  program at its next input, so the process is retired at once and the next statement runs on a
  fresh one; so is a process whose statement was left unread. The statement timeout of a connection type works the same way. On Windows there is
  no interrupt: the session is closed instead.
- **Files**: `database` holds the path; a relative one in a project connection is resolved against
  the project's folder when it connects (`zen_workbench::database::files::resolve`). Connecting
  never creates a file (`mode=rw`; a missing file is `fileNotFound`). `db.databaseFile.create`
  creates an empty file (an empty file is a valid SQLite database) and refuses one that exists;
  `db.databaseFile.open` finds the connection that opens a file or answers the fields of a new one.
  The connection dialog shows Path with **Open…** and **Create…** for a driver with a shared
  connection, no SSH/SSL page, and a **Browse…** button for `path` settings (the `program`).
- **The program** is found from the connection's `program` setting, else `PATH`, else
  `/usr/bin`, `/opt/homebrew/bin`, `/usr/local/bin`; versions before 3.37 (the first with `-safe`; its `-version` is asked before it starts) are refused with code `sqliteMissing` and the version found; output of every version from 3.37 on is read, also values with line breaks, which `sqlite3` before 3.52 writes raw. Without it
  connecting fails with code `sqliteMissing` and a message naming what to install.
- **Editing** writes values as quoted literals instead of `.parameter set` (ADR-0170): results of a
  plain `SELECT` of one table are traced to it, as `sqlite3` does not name a column's table.
- **Not yet:** column types of results are not known (above).
- **Tests**: unit tests of the scanner, the literals and the framing, and integration tests in
  `crates/platform/db/tests/sqlite.rs` against the real `sqlite3` (they pass with a message when it
  is not installed).

## Redis, Valkey and ClickHouse

Of the next databases of ADR-0164, Redis/Valkey and ClickHouse are built in; MongoDB and SQL Server
are not compiled in because of their TLS (decision 4: `mongodb`'s `rustls-tls` turns on `ring`,
`tiberius`'s `rustls` takes `tokio-rustls` with aws-lc) and are not forked. Both now come as
external driver programs that speak TLS themselves (ADR-0175, [below](#first-party-driver-programs)):
SQL Server over `tiberius` without TLS, MongoDB as a client of our own.

- **Redis and Valkey** (`zen_db::redis`): `redis` 1.8.0 with `default-features = false` and
  `tokio-rustls-comp`, `tls-rustls-insecure` (for TLS mode `require`, certificates unchecked; pure
  Rust), `cluster-async`, `sentinel` and `streams`. rustls takes the process's RustCrypto provider,
  installed once by the driver (`CryptoProvider::install_default`); roots are the system's
  (`rustls-native-certs`) or the connection's CA file, with a client certificate when given.
  - **Targets.** `mode` (`standalone`, `sentinel`, `cluster`; a `redis+sentinel://` URI chooses
    Sentinel), hosts as `host[:port][,host[:port]…]`, the database as `n` or for Sentinel
    `primary[/n]` (or the `master` setting). RESP3 first, RESP2 when the server has no `HELLO`.
    `rediss://` or an SSL mode from `require` turn TLS on (`verify-ca` skips the host name, as does a
    connection through an SSH tunnel; a CA file with `verify-ca` still checks the name, a limit of
    the crate). Responses have no client timeout of the crate's (its default is 500 ms); the engine's
    deadlines apply.
  - **The tree.** Logical databases from `INFO keyspace` plus the configured one (`db0`, with their
    key counts); in a cluster its primaries from `CLUSTER NODES`, each read by routing to its
    address. Under them keys grouped by the `separator` (`:`): a prefix node's last path segment
    keeps the separator (`["db0", "user:", "1"]` is `user:1`), so a key and a prefix of the same name
    never collide and the key is the concatenation of the path. `SCAN … MATCH <prefix>* COUNT 1000`
    up to 10 000 keys or 200 calls per level, then a `more` node; types with one pipelined `TYPE` per
    key (1000 at most). Never `KEYS` (the integration test checks `INFO commandstats`).
  - **Values** (`DriverSession::key`, `zen_db::value::KeyValue`): `TYPE`, `PTTL`, `MEMORY USAGE`,
    `OBJECT ENCODING`, then per type: a string's first 1 MiB (`GETRANGE`; JSON-looking text as JSON),
    `HSCAN`, `LRANGE`, `SSCAN`, `ZRANGE … WITHSCORES`, `XRANGE … COUNT`, `JSON.GET` for the JSON
    module, at most `limit` items with the full length. Values are never kept in snapshots.
  - **The console** splits a line as `redis-cli` does (`zen_db::redis::line`: double quotes with
    escapes and `\xHH`, single quotes); replies become values: a map as field and value rows, an array
    as numbered rows, a scalar as one `reply`. `SUBSCRIBE`, `MONITOR` and the like are refused (no one
    reply). `SELECT` is followed; `MULTI`…`EXEC` counts as an open transaction (Commit runs `EXEC`,
    Rollback `DISCARD`); the driver has no `transactions` capability, so the console toolbar shows no
    commit mode. Cancel opens a second connection and sends `CLIENT UNBLOCK <id> ERROR` (blocking
    commands stop at once; anything else ends the session as the engine does for an unconfirmed
    cancel). The client name is the guard's application name with spaces as dashes.
  - **Read-only** is the client's: before a command runs, `COMMAND INFO` (cached per command) says
    whether it has the `write` or `may_replicate` flag; administrative changes (`CONFIG SET`,
    `FLUSHALL`, `SHUTDOWN`, `CLIENT KILL`…) and scripts (`EVAL`, `EVALSHA`, `FCALL`; the `_RO` forms
    run) are always refused; without `COMMAND` a list of reading commands decides. Code `readOnly`.
  - **Structure changes**: `DEL`, `UNLINK`, `RENAME`, `FLUSHDB`, `MOVE`, `COPY`, `SWAPDB`, `RESTORE`
    refresh the tree.
- **ClickHouse** (`zen_db::clickhouse`): its HTTP interface through `ureq` with rustls and the
  RustCrypto provider set on the agent's TLS config (ADR-0040), not `zen-http`'s `send` (which reads a
  whole body) and not the `clickhouse` crate. Every statement is a POST with `session_id` (one per
  execution context, so `SET` and `USE` last), `database`, a fresh `query_id`, `default_format =
  JSONCompactEachRowWithNamesAndTypes` (chosen over RowBinaryWithNamesAndTypes: text per line,
  names and types first, parsed with `serde_json` as it arrives, exact decimals and 64-bit integers
  as quoted strings), `readonly=2` on a read-only context and `max_execution_time` from the
  connection type's statement timeout. User and password go in `X-ClickHouse-User`/`-Key` headers,
  never in the URL; names in the tree's queries are query parameters (`{database:String}`). The body
  is read on the blocking pool and handed over in pages of 200 rows; the result store asks for rows
  as with PostgreSQL, and an error inside the stream fails the statement. Cancel sends
  `KILL QUERY WHERE query_id = {id} ASYNC`. Values map by type (`Nullable`, `LowCardinality`,
  arrays, tuples, maps, decimals, dates as the server writes them, UUID, IPs as other). Its tree is
  the driver's own (`children`): databases (system ones flagged), the folders Tables, Views and
  Dictionaries with engine, rows and bytes, then columns; `Driver::open_text` writes the console's
  `SELECT`.
- **Opening nodes of a driver's own tree** (`db.object.open`): a key opens its value tab
  (`db.key.open`, a workspace tab of kind `key` drawn by `ide/database_view/key_view.rs`: type, TTL,
  length, memory, encoding, Set TTL, Persist, Refresh, the items in the data grid or a string as
  text); a ClickHouse table or view a console with the driver's `SELECT`. Copy Name of a key copies
  the whole key.
- **Snapshots** of drivers' own trees keep each node's system flag, size, rows and comment (format
  version 3, and 4 since the profile in effect is kept too; older files are read again), so system databases hide with system objects off.
- **Tests**: unit tests of line splitting and quoting, replies, grouping, targets and ClickHouse
  values and requests against a local fake HTTP server; ignored integration tests against Redis 7
  and 8, Valkey, a three-node Redis cluster, a Sentinel and ClickHouse in Docker
  (docs/development.md).
- **Not yet**: editing values (adding fields or members, renaming and deleting keys from the tab),
  completion of keys the tree has not read (a bounded `SCAN` by prefix), server info and slow log
  tabs, ClickHouse's structure in the relational snapshot (DDL, `db.object.get`; completion reads its
  tree as tables, see below).

## Object editors

- **Where.** A database object opens as an inner tab of the Database workspace (kind `object`),
  coloured along its bottom by the connection type, like consoles. Under the tab strip a row of pages:
  **Properties · Data · DDL · Diagram** for tables, views, materialised views and foreign tables,
  opening on Properties from the navigator (ADR-0192) and on Data through `db.object.open` without
  a page; **Properties · DDL** for functions, procedures, sequences, types and schemas,
  opening on Properties. On the right of the row: the qualified name and the connection.
- **Data** is the results grid (`ide/data_grid.rs`) over a result of the result store: the table read
  with `SELECT * FROM <schema>.<name>`, in the database's order (no
  `ORDER BY` of the key: the owner's decision of 2026-10-28; a sort from a header adds one), in the connection's **Main** context, so it never takes a console's
  transaction; the first page at once, the rest as consoles read. It reads by itself only when the
  connection is connected; otherwise the page says so and **Read Data** connects (the person's
  action). The bottom bar, Text and Record views, the value panel, the **WHERE filter** box and
  staged edits are the console's (next section); a filter or sort goes into the generated `SELECT`.
- **Properties**: a header (name, comment, owner, tablespace; a routine's arguments, result and
  language) and vertical sections, each with its count (a table section's rows): Columns (no
  position column: the grid numbers the rows), Constraints, Foreign Keys, Indexes,
  Triggers, References (other tables' foreign keys to it), Policies, Partitions (the key, or the
  parent and bound, and the partitions), Statistics (row estimate, sizes, indexes) and Permissions
  (grantee, privileges, `*` for grantable, grantor) for tables; views and materialised views show
  their query as Source; routines their source; sequences and types their settings; a schema its
  objects. What a driver's databases do not have is left out (`objects::applies`: SQLite and DuckDB
  no owner, tablespace, policies, partitions or permissions; MySQL and MariaDB no owner or policies;
  ClickHouse no tablespace, policies or permissions). The grid's last column first fits the room
  left (`GridState::fit_last`), so a long definition ends with "…" in view, its whole text in the
  cell's tooltip. All from the snapshot, offline; an object whose details were not read is read when
  the connection is connected.
- **The Overview's rows** have a status column and Edit and Connect/Disconnect buttons of fixed
  widths, measured for the longest label of the language, so the buttons of every row line up; the
  name and address end with "…" before the status; when less than 140 px is left for them the row
  takes two lines (the name, then the state and address), and a header too narrow for its title and
  buttons puts the buttons on the next line.
- **DDL**: the offline generator's statements, read-only, selectable and coloured, with the six
  toggles, **Copy** and **Open in Console** (a new console of the connection with the text). The
  tablespace of a table not in the default one is in its DDL; a schema's DDL is `CREATE SCHEMA …
  AUTHORIZATION …` with its comment.
- **Diagram**: the table's ER diagram (next section).
- **Kept** in `workspace.json` with the tab: the connection, path, page, section and DDL toggles,
  never rows; after a restart the tab comes back without connecting. Closing the tab frees its rows.
- **Not yet:** views' triggers and policies other than PostgreSQL's; per-column statistics.

## ER diagrams

- **Model** (`zen_db::diagram`, pure, also for WebAssembly): `of_schema` takes a schema's tables,
  views, materialised views and foreign tables (not partitions, not system objects) and the tables of
  other schemas they reference (`external`); `of_table` takes a table or view, the tables it
  references and those that reference it, with only the foreign keys that touch it. Entities are
  sorted by (external, schema, name); each column says whether it is in the primary, a foreign or a
  unique key. A relation is `optional` when a column of its foreign key is nullable and `unique` when
  its columns are a primary or unique key. Entities whose details were not read have no columns
  (`partial`).
- **Layout** (`zen_db::diagram::layout`): deterministic, cancellable through an `AtomicBool`. Boxes
  are sized from character counts (`Metrics`: column rows are monospaced, so the estimate is exact
  enough; at most 40 rows, then a `+N` row); connected groups are laid out in columns by the longest
  chain of foreign keys (referenced tables left, cycles broken by name order), ordered by four
  barycentre sweeps and placed near their neighbours; groups are packed on shelves, largest first,
  and entities without relations go into a grid after them. Lines leave the referencing row and enter
  the referenced row, with one vertical segment between columns; lines inside one column go round the
  right side. 3000 tables lay out in well under a second.
- **Exports**: `svg` (colours given by the caller, `Colors`) and `mermaid` (`erDiagram`; names that
  Mermaid cannot take are made identifiers with the original as an alias or comment; `}o--||`,
  `}o--o|`, `|o--||` from `unique` and `optional`).
- **Workbench** (`zen_workbench::database::diagrams`): a schema's diagram is an inner tab of kind
  `diagram` (`diagram:<connection>:<catalog>/<schema>`, data `{ connection, path, view }`); a table's
  is its object tab's Diagram page (`ObjectTab::diagram` keeps the view). Each tab's diagram is built
  from the snapshot on the UI thread only when the structure's revision changed, and laid out on a
  thread of its own when it differs from the last one; the previous layout stays shown meanwhile.
  When some entities lack details and the connection is connected, their details are read (a schema
  never read, or up to 500 objects at a time); nothing connects. The view (zoom in percent, scroll
  position in diagram points) is kept in `workspace.json`. Exports to a file run on a thread of their
  own (written to `.part`, then renamed; an existing file only with `overwrite`); PNG and SVG colours
  come from a `Renderer` the window installs, which draws with the current theme.
- **Canvas** (`zen_ui::diagram`, for any feature): a `Scene` of boxes (title, rows with a key marker,
  dashed or muted) and polylines with one / zero-or-one / many ends, a `Camera` (zoom 10 % to 400 %
  and origin), `paint` (only what is in view; no text under 42 %), `show` (drag or scroll to pan,
  ⌘-wheel or pinch to zoom around the pointer, click, double click) and `render` (the whole scene
  offscreen for PNG export). Colours are the palette's: canvas `editor_background`, boxes `island`,
  headers `sticky_background`, outlines `button_border`, keys `warning_text`, foreign keys `link`,
  the selection `accent`.
- **Window** (`database_view/diagram.rs`): the bar (−, +, the current zoom as a button that resets to 100 %, Fit
  with the platform's fit icon, Export with a dropdown chevron, the counts, Cancel while laying out) over the canvas; the navigator's context menu has **View Diagram** for
  schemas, tables and views. The camera is the view's, synchronised with the tab's kept view through
  `db.diagram.view`.
- **Not yet:** editing (moving boxes by hand and keeping their places, notes), diagrams of several
  schemas or chosen tables, showing indexes or comments, routing lines around boxes, and a printable
  page layout.

## The value panel and Calc

- **The value panel** beside every result grid (Value Panel in the bottom bar, ⇧⌘P in the Database
  workspace, `db.result.toggleValuePanel`; its width is kept) shows the selected cell of the grid or
  the record view: its column and type, the formats the value offers — **Text** (arrays as SQL
  literals), **JSON** (pretty, keys in their order, foldable by the arrows in its gutter, for JSON,
  documents, maps, arrays and text holding JSON), **XML** (indented, foldable), **Hex** (a dump of
  bytes or text) and **Image** (bytes holding a PNG, JPEG, GIF, WebP, BMP or ICO picture, decoded on a
  thread of its own) — with **Copy** and **Save…** (bytes as they are, text in the format shown).
  The format picked is remembered and used whenever a value offers it. Values of any driver are
  shown: documents, maps and arrays too. Timestamps are shown as the server sent them, with the
  zone.
- **Calc**, under the value: count (not NULL), distinct, sum and average of the numbers (integers
  exactly), minimum and maximum of the selection, worked out on a thread of its own, at most a
  million cells; the status bar shows count, sum, average and the range while more than one cell is
  selected.
- **The record view** shows each field whole (up to 2000 characters, JSON pretty), wrapped; a click
  selects the field, so the value panel and Calc follow it.
- **The geometry viewer** (without a map): a spatial value
  offers **Geometry**, its preferred format, and the panel draws it as shapes on a canvas with a grid
  and its coordinates, together with every other spatial value of the selection (up to 2000 cells),
  the current cell in the accent colour and the others in the secondary text colour, areas filled
  with their holes; scroll zooms around the pointer, dragging moves, a double click fits. The caption
  gives the kind, the SRID and the points. Values read: PostGIS `geometry` and `geography` (EWKB, as
  the binary protocol sends them), MySQL's geometry (SRID and WKB), GeoPackage blobs, WKB, WKT and
  EWKT text (`SRID=4326;POINT(…)`), and GeoJSON (a geometry, a feature or a feature collection);
  arcs (`CIRCULARSTRING`, curve polygons) are cut into segments. `zen_db::geometry` parses them in
  pure Rust; the shapes are read on a thread of their own, cancelled when the selection changes,
  and projected again only when the view changes. **No map tiles**: nothing is fetched, the IDE
  stays offline. Copy and Save… give the WKT (`.wkt`). `db.result.geometry` lists the shapes of a
  selection for agents.
- **Not yet:** editing the value in the panel and returning it to the cell; cells are edited in the
  grid (next section). The geometry viewer: SpatiaLite's own blob format, PostgreSQL's built-in
  geometric types (`point`, `box`, `polygon`), ClickHouse's geo types, projections between SRIDs
  and an optional online base map.

## Filtering, sorting and editing data

- **The WHERE filter box** sits above every data view: an object tab's Data page and every console
  result. It takes an SQL condition as written after `WHERE`; ↩ applies it, ⌃Space or typing a word
  completes column names (quoted as SQL needs, with their types) and the keywords AND, OR, NOT, IS,
  NULL, BETWEEN, LIKE, ILIKE, IN, TRUE, FALSE, EXISTS. ‹ and › go back and forward through the
  filters used, ▾ lists the table's **history** (20, newest first, kept per connection and table in
  `<data folder>/databases/filters.json`) and its **saved filters** (named, standard), ✕
  clears the filter and the sort, ⌥⌘F puts the keyboard in the box. The statement the rows were
  read with is shown beside the box (whole in its tooltip).
  A filter (or a reading of the rows) that fails outlines the box in the error colour, shows the
  error beside it and puts it in place of the grid, so the rows read before are never shown as the
  filter's; `db.result.filterState` answers the error for object tabs as for consoles.
- **How a condition goes into SQL, safely.** The condition is first checked to be **one balanced
  expression** by our own scanner (`zen_db::filter`): quotes, `E''` strings, dollar-quoted bodies and
  comments must close, parentheses must match, and a `;` outside a literal is refused — so the text
  cannot end the statement or leave its parentheses. It is then always put in parentheses
  (`WHERE (…)`, a trailing comment gets a line break before the `)`), and only at a clause
  boundary found by the same scanner on the base statement: for an object tab, into the generated
  `SELECT * FROM schema.table WHERE (…) ORDER BY …` (no `ORDER BY` without a sort); for a console
  result whose statement is a plain `SELECT` (one top-level `FROM`, no `UNION`, `GROUP BY`,
  `HAVING`, `WINDOW`, `LIMIT`, `OFFSET`, `FETCH`, `FOR`, `INTO`, `WITH`), **merged**: an existing
  `WHERE x` becomes `WHERE (x) AND (…)` and `ORDER BY` is replaced by the sort; any other query that
  only reads is **wrapped**: `SELECT * FROM (\n<query>\n) AS zen_filtered WHERE (…) ORDER BY …`.
  A statement that does not only read (DML, `RETURNING`, `SHOW`, `CALL`) is never re-run with a
  filter: its result can only be sorted and searched in memory. The filter is always composed from
  the statement as first run, never from an already filtered one, and it runs in the same session
  (Main for object tabs, the console's own), so it sees that session's transaction and read-only
  setting. Every generated piece is written in the driver's dialect (ADR-0170,
  `zen_db::dialect::SqlDialect`): identifiers quoted as the server needs, quick-filter values as its
  literals (MySQL text with a backslash as `_utf8mb4 X'…'`, bytes as `X'…'`), contains as
  `LIKE '%…%' ESCAPE '!'` on the column cast to text (ClickHouse: `toString` and backslash escapes),
  and the filter box read with the dialect's quotes and comments.
- **Applying** reads the rows again in the background as a query of their session (cancellable from
  the bar, `db.query.cancel`, shown as Filtering… for object tabs and consoles alike); the rows shown
  stay until the new ones have their columns, then replace them in the same tab, and the old rows
  are freed. Staged edits are never dropped silently: filtering, sorting, reading again, a console
  run that replaces unpinned result tabs, and closing a result tab, an object tab or a console are
  refused with code `pendingChanges` naming the results and the command to run again, and the
  window asks **Save / Discard / Cancel** (Save runs the usual save, with the
  preview to confirm where the connection type asks, and runs the command once it is done; Discard
  reverts and runs it; Cancel keeps everything). Nothing can be staged on rows that are being
  replaced (code `reading`): the grid is busy while it reads.
- **Sorting.** The ⇅ mark at the right of a column header sorts by it: a click cycles ascending,
  descending, none; ⇧-click adds the column to the sort (its place is numbered). When every row is
  loaded the rows are sorted **in memory** (NULL largest; staged edits move with their rows);
  otherwise the rows are read again with `ORDER BY` (by name, or by position when two columns share
  a name) with `NULL`s placed the same way on every server (`NULLS LAST`/`FIRST`, or `col IS NULL`
  first on MySQL and MariaDB). A header click elsewhere still selects the column.
- **Quick filters** from a cell's context menu, **Filter ▸**: `= value`, `<> value`, `> value`,
  `< value`, `LIKE '%value%'`, `IS NULL`, `IS NOT NULL`, IN the selected rows' values, `=` the
  clipboard's text, a custom value (starts `"column" = ` in the box), **By Values…** (the distinct
  values of the rows loaded with their counts, checked into an `IN`), and **Reset Column Filter**
  (removes the top-level `AND` parts naming the column). Each is added to the condition with `AND`.
  The type mark of a filtered column is drawn in the accent colour.
- **Find** in the rows loaded (`db.result.find`) needs no server: cells containing the text, in
  memory.
- **Editing** stages edits until Save. A result is editable when its rows come from
  **one table** (the server's row description names each column's table: PostgreSQL's table oid and
  column number, kept as `Column.source`, so wrapped and merged filters stay editable; MySQL's and
  MariaDB's column metadata name the database, table and column, kept as `Column.origin` with
  `dialect::table_id`; SQLite's results are traced from a plain `SELECT` of one table), on a writable
  connection, with a driver that edits (`Capabilities.row_identity` and an SQL dialect that can:
  ClickHouse answers `noRowIdentity`, key-value drivers `driver`), and a **row identifier**: the
  primary key, else a unique key of non-null columns, else PostgreSQL's `ctid` read with the rows,
  else a **virtual key** chosen once and kept for the table
  in the project's `.zen/databases/virtual-keys.json`, else the **No Unique Key** dialog: use the
  selected columns (and keep them) or all columns (warned: rows equal in every compared column cannot be told apart, so saving a
  change that would touch several of them is refused and rolled back, `severalRows`) or cancel.
  Otherwise the bar under the grid always says why the result is read-only, after a **Read-only**
  chip with a lock; an attempt to edit (a double-click, ↩, F2 or typing) turns the reason red for a
  few seconds instead of doing nothing, and the context menu keeps its edit items, disabled, with the
  reason as their tooltip. **Set to NULL** is disabled on a `NOT NULL` column.
- **In the grid**: ↩, F2 or a double-click edits a cell in place, and typing a character starts
  editing with that character in place of the value (the editor keeps its cell's width) (typed values are checked against the
  column's type: numbers, booleans, JSON, UUIDs, bytes as `\x…`; dates and times are text the server
  reads); Esc leaves the editor, and on a cell drops its change; the context menu sets NULL or the
  default, copies from the row above, adds (⌘N), duplicates (⌘D, keys and identity columns left to
  their defaults) and deletes rows (⌘⌫; deleted rows stay struck through until saved). Changed cells,
  new rows and deleted rows are tinted with the theme's diff colours and marked in the row numbers;
  the tab shows as modified. Generated and computed columns are read-only. A changed cell
  also carries a bar of the gutter's modified colour at its left, so it stays visible when selected;
  the strike-through of a row to delete ends at the last column.
- **Preview SQL** (the bar's Preview…, `db.result.previewChanges`) shows the statements Save runs:
  deletes, then inserts, then updates, one per row; an `UPDATE` sets only the changed columns, finds
  its row by the **old** key and checks that each changed cell still holds the value read
  (`IS NOT DISTINCT FROM`, `<=>` on MySQL, `IS` on SQLite; JSON and other types compared as text;
  MySQL `FLOAT` cells not compared), and the saved row is read back: `RETURNING` on PostgreSQL and
  SQLite, a `SELECT` by key after the statement on MySQL and MariaDB (`LAST_INSERT_ID()` for a new
  auto-increment key). Values are **real bind parameters** on PostgreSQL (`$1`…, sent as text the
  server parses as the column's type) and MySQL/MariaDB (prepared statements, `?`; bytes through
  `UNHEX(?)`; booleans written as `TRUE`/`FALSE`), never written into the SQL; SQLite gets safely
  quoted literals (ADR-0170). A MySQL `UPDATE` counts the rows it matched, so an unchanged value is
  no conflict. The preview writes values as literals for reading. Where the connection type confirms data changes (Test, Production),
  Save always opens the preview first and saves on its Save.
- **Save** (⌘S in the grid or the Database workspace, `db.result.save`) runs in the result's own
  session in the background: auto-commit — `BEGIN`, the statements, `COMMIT`; inside an open
  transaction or in manual commit (Production) — `SAVEPOINT zen_save` … `RELEASE`, leaving the changes
  in that transaction for its Commit or Rollback. A statement
  that changes no row is a **conflict** (the row was changed or deleted by someone else) and, like any
  error, rolls everything back (to the savepoint) and keeps the changes staged; the bar says which row
  and why. **Cancel** (⌥⌘Z, `db.result.revert`) drops every staged change.
- **Generate SQL ▸** SELECT, INSERT, UPDATE or DELETE of the selected rows, by key (or every column),
  values as literals, copied to the clipboard (`db.result.generateSql`).
- **Not yet:** editing in the value panel; staged edits come back after a crash as SQL to review,
  not staged again in the grid;
  lazy reading of huge cells by key; a new row is added after the rows, not under the selected one;
  ⌘N, ⌘D and ⌘⌫ are the grid's own keys (the keymap lists Add Row without a key: in consoles they stay
  New Scratch, Duplicate Line and Delete Line); the column filter's custom value is typed in the box,
  not in a dialog; By Values counts the rows loaded, not the whole table.

## Following foreign keys

- **Which results link.** A result whose rows come from one table known to the snapshot: an object
  tab's Data page (its table, columns matched by name), or a console result whose columns the server
  traces to one table (PostgreSQL's table oid and column number, MySQL's and MariaDB's origin, SQLite's
  traced plain `SELECT`). The table's details are read on demand when the snapshot holds only its
  name (`db.result.relations` answers `reading` meanwhile). Its foreign keys whose columns are all in
  the result are **links** (`zen_db::relations::outgoing`; a SQLite key that names no target columns
  points at the target's primary key); other tables' keys that point at it, whose target columns are
  all in the result, are **references** (`incoming`). Results of several tables, of no table, and of
  drivers without SQL (Redis) have neither.
- **In the grid** the cells of link columns are drawn in the theme's link colour; pointing at one
  shows an arrow at its right: a click on it, ⌘-click, ⌘B or **Open Referenced Row (table)** in the
  menu opens the referenced table's Data page as an object tab (or brings its tab to front) filtered
  by the key: `col = value AND …` for every column of a composite key, identifiers and literals in the
  driver's dialect (`zen_db::relations::key_condition`, ADR-0170); a key with a NULL points nowhere
  (`nullKey`). The condition goes into the tab's filter like a typed one: the table's filter history,
  the filter's own ‹ ›, and Save / Discard / Cancel when that tab has staged changes. **References ▸**
  lists `schema.table (columns)` of the referencing keys and opens that table filtered the other way.
  Reads run in the background in the Main context and are cancellable as any data read.
- **Back and Forward** (← → at the left of the bar under the grid, `db.data.back`, `db.data.forward`)
  walk the data tabs visited by following keys in this window, browser-like: Back brings the tab you
  came from to front and, when its filter changed since, reads it again with the filter it had; a
  closed object tab opens again, a closed console is dropped (`tabClosed`). Each table keeps its
  own tab (its editing, filters and key choice stay its own), and Back and Forward move between those
  tabs. The history holds 50 steps and is not kept across restarts.
- **Choosing a key while editing** (a dictionary lookup): ⇧↩ on an editable key cell or
  **Choose Value…** opens a list of the referenced table's rows: `SELECT * FROM <table> [WHERE <key or
  text column> LIKE '%…%' ESCAPE '!' OR …] ORDER BY <its key> LIMIT 201`, read in the background in
  the Main context and cancelled when the search changes or the list closes; up to 200 rows, the key
  columns first, at most eight columns; the search covers the key and up to six text columns. A click
  stages the row's key into every column of the foreign key (values checked against each column's
  type); nothing is written until Save.
- **Not yet:** the references panel beside the grid (Panels ▸ References, rows of every
  referencing table at once); keys of virtual (user-declared) foreign keys; following keys from the
  record and text views; showing a description column instead of the key in the grid.

## The data grid

Decisions of 2026-10-28 (the owner: the value panel open by default and no `ORDER BY` of the key):

- **The value panel is open by default** (`DbSettings.value_panel` defaults to `true`); hiding it is
  remembered as before.
- **Table data in the database's order.** The Data page reads `SELECT * FROM <table>`; a header's
  sort adds `ORDER BY`. Fetch Next past the row cap reads the statement again and skips the rows
  kept, so on a table that changes in between, rows read in two runs may repeat or be missed
  without an order; sort by the key when that matters.
- **Header marks and the column menu.** A key mark (`IdePalette::accent`) on the columns of the
  primary key that identifies the rows (`db.result.editState` `key.kind = primaryKey`), a link mark
  (`IdePalette::link`) on columns whose foreign key the relations know; the header tooltip names
  them. A **▾** left of the sort arrows opens the column's menu (sort ascending, descending, remove
  sorting; custom filter, by values, reset; select column, fit width, copy name). A click on the
  name still selects the column, as before.
- **The context menu is grouped**: Copy ⌘C, Advanced Copy ▸ (with header, column names, row as
  JSON, Copy As ▸, Advanced Copy…), Paste ⌘V, Advanced Paste ▸, Edit ▸ (F2, ⇧↩, ⌘N, ⌘D, ⌘⌫ shown),
  Filter ▸, Order ▸, Navigate ▸ (referenced row ⌘B, References ▸, Go to Row… ⌃G, First Row ⌘↑,
  Last Row ⌘↓, Fetch Next, Fetch All), View/Format ▸ (Dense Rows), Panels ▸ (Value Panel), Layout ▸
  (Grid, Text, Record), Generate SQL ▸, Export Data…. Hints, a logical structure view, row colours
  and column transforms are not offered yet.
- **The bottom bar** gains first row, page back, page forward (reading the next page when the
  selection nears the end) and last row read, an editable **fetch size** box (`db.settings.update
  { page }`, 1–10 000) and **Calculate Total** (`db.result.count`) while the total is unknown; in a
  narrow window Calculate Total moves into the ⋯ menu with Fetch Next, Fetch All and Export.
- **Go to Row** (⌃G in the grid, Navigate ▸) is `db.result.select { id?, row, column? }`: rows from 1,
  refused with `noRow` past the rows read (the answer says how to read more, so nothing is read
  without asking). The window takes the wanted cell each frame
  (`values::take_selection`).
- **Grid/Record**: ⇥ in the grid shows the record, ⇥ in the record (nothing focused) shows the grid
  (`db.result.view`). The views are buttons in the bottom bar.
- **Filter by value and history** were there already: Filter ▸ starts with `column = value`, and the
  filter box's ▾ lists the table's history and saved filters.
- **Result tabs** are named `<table> <run number>` (`orders 3`, `orders(+) 4` for a join); quotes are dropped from the name, and string literals and comments no longer
  confuse finding the table (`results::table_of`).

## Query history

- **What is recorded.** The engine reports every statement it runs (`zen_db::history::Ran`, through
  `Engine::set_history`): each statement of a query (`QueryOptions::provenance` says what ran it:
  `statement` for ⌘⏎, `script` for ⌥⌘⏎ and Execute from Caret, `filter` for a filter or sort, also on
  an object tab, `data` for an object tab's rows, `query` for `db.query.run`), each statement of a
  saved edit (`Engine::save_edits_from`, `save`) and Commit or Rollback of an open transaction
  (`Engine::end_transaction_from`, `transaction`), and the query or table each item of an export reads
  (`Engine::export_from`, `export`, its place the item's name), and the scripts of structure changes
  (`structure`, its place the object's name). `agent` is set when the command came from an agent
  (`zen_core::macros::by_agent`). Paging (Fetch Next re-reads), row counts, introspection and the
  IDE's own control statements (`BEGIN`, savepoints) are not recorded.
- **Where.** One journal per data folder, `<data folder>/databases/history.jsonl`, shared by every
  window of the process (`Journal::shared`) and written by one thread: an append-only file of JSON
  records, each line `<CRC-32 in hex> <JSON>` (`zen_db::journal`), synced at most once a second
  (`File::sync_all`, a full sync on macOS), read at start (a torn line or one whose check fails is
  dropped and the file rewritten; lines without a check, from before, are read and rewritten with
  one), rewritten through a temporary file, a rename and a sync of the folder when pruning leaves
  more than a tenth of dead lines, and deleted when clearing leaves nothing. Settings are in
  `history-settings.json` beside it: on/off, 90 days, 10 000 statements; text is capped at 64 KiB a
  statement and 64 MiB in all. Separate IDEs have separate data folders, but an IDE and a headless
  one of the same product (`mcp --headless`) share theirs and both append to this file; a rewrite
  by one of them leaves the other appending to the replaced file until it opens it again, so lines
  can be lost then (a known gap; the recovery journal below has one file per process).
- **Secrets.** Values of bind parameters never reach the text (saves send them bound). Passwords
  written in statements are masked before anything is kept, in memory or on disk
  (`zen_db::history::redact`: `PASSWORD '…'`, `PASSWORD = …`, `PASSWORD('…')`, `SET PASSWORD FOR … =`,
  `IDENTIFIED [WITH plugin] BY …`, `password=` in connection strings, URI passwords, Redis `AUTH`,
  `HELLO … AUTH`, `MIGRATE … AUTH/AUTH2`, `CONFIG SET requirepass/masterauth`, `ACL SETUSER >…`), and
  the entry is marked `redacted`; such an entry is not run again as it is. Error details are masked
  the same way. Values written as literals in ordinary statements (`WHERE email = '…'`) are kept: they
  are what the person typed.
- **Search** is in memory: each entry keeps its text and error lowercased, and a search checks that
  every word is contained (10 000 statements in well under 30 ms, checked behind
  `ZEN_TIMING_CHECKS`), with filters by connection, source, agent, outcome and time.
- **The tab** (`ide/database_view/history.rs`, kind `history`, opened by `db.history.open` and the
  toolbar's clock button): a search box, connection, type, result and date filters, the count and ⚙
  (record on/off, days, most statements, Clear History… with a question); the list newest first,
  virtualised; the chosen statement whole below with Open in Console (also a double-click), Run
  Again and Copy, also in each row's context menu. It reads the history again only when the
  journal's revision or the filters change.
- **Not yet:** per-console history navigation (no key yet); the History tab has no key of
  its own; opening a history entry restores neither the console's schema
  nor its transaction mode.

## Database objects in Search Everywhere

- **Where.** A **Database** tab of Search Everywhere and its group in All
  (`zen_workbench::database::search`, a `SearchContributor` the Database module adds to each window),
  and **Go to Database Object** (⌘O in the workspace's keymap context, `view.gotoDatabaseObject.open`),
  which opens the popup on that tab. `db.object.goTo` answers
  the same search for agents.
- **What it searches.** The names of every connection's snapshot: schemas, objects of every kind
  and columns, and for drivers with their own tree (Redis, ClickHouse) the nodes read (keys, tables,
  collections; prefixes and folders are not objects). System objects and schemas are left out, as
  the navigator hides them by default. Then the scripts and consoles of the connections
  (`db.script.list`) by file name, then the query history by words (its own in-memory search).
  Nothing is asked of a server: a snapshot not loaded yet is read from disk on its thread
  (`structure::ensure`) and the tab says it is reading until it comes.
- **Fast without a server.** The names are flattened into one list per window (`NameIndex`: the
  name, its kind and an owner holding the connection, the detail and the path to open), kept in the
  session and built again only when the structure's revision or the connections change. Matching
  uses the platform's `NamePattern` (camel humps, `*`, word starts), the same as Go to Class;
  `qualifier.name` matches the qualifier against what holds the name (a schema for an object, a table
  for a column, the database for a schema), never in the middle of a word. Only the best `limit` are
  sorted (`select_nth_unstable`). Measured: 100 001 names (1000 tables of 99 columns) in 0.8–1.2 ms
  in a release build, 17–21 ms in a debug build; the 10 ms budget is asserted only with
  `ZEN_TIMING_CHECKS`.
- **Off the window's thread, cancellable.** The popup's search runs on a thread of its own with the
  index shared (`Arc`), checks for cancellation every 4096 names and stops when a newer query
  replaces it; the tab answers `Pending` until it is done. Building the list (only after a snapshot
  changed) and listing the scripts happen on the window's thread.
- **Order.** By how the name matches (exact, prefix, humps, middle), objects before columns, then the
  matcher's degree and the qualifier's match; scripts after objects, statements of the history last.
- **What it shows.** The name with the matched characters, the place (`schema`, `catalog.schema`
  when the connection lists several databases, `schema.table` for a column, a script's place) and
  the connection's name, and the kind on the right; the icon is the navigator's glyph of the kind.
- **↩ opens it.** The item's target is a command (`SearchTarget::Run`): `db.object.open` (a column
  opens its table on Properties, whose first section is Columns), `workspace.tab.open` for a script,
  `db.history.openInConsole` for a statement; each expands the Database workspace.
- **Not yet:** selecting the column found in the table's Columns section; the navigator's Custom
  view filters do not narrow the search.

## Export

- **One dialog** (`ui::ide::export_dialog`, every setting on one page):
  from **Export…** under a result, **Export Data…** in the navigator (a table or view; a schema or a
  folder lists its tables to choose, each to its own file) and **Export From Query** in a console's
  toolbar. Commands `db.export.settings`, `.start`, `.state`, `.cancel`
  ([automation.md](automation.md#export)).
- **Formats** (`zen_db::export`, one row at a time into any `Write`, so Copy As can reuse them):
  CSV (delimiter, quote, header, quote all, NULL text, formulas escaped by default: text starting
  with `=`, `+`, `-`, `@`, a tab or CR gets a `'`; empty text quoted apart from NULL), TSV, JSON (an
  array or JSON Lines; numbers, booleans and JSON as themselves, everything else as strings, bytes as
  `\x…`), SQL `INSERT` in the connection's dialect (`SqlDialect::literal`, batches of N rows, the
  target table as typed or the source's), Markdown, HTML (escaped), XML (`<row><value column=…>`,
  NULL as `null="true"`, characters XML 1.0 cannot hold replaced). LF or CRLF, UTF-8 with an optional
  BOM. Values are written as the grid shows them (dates as the server sent them, arrays as their SQL
  literal).
- **Streamed, never through the result store**: the query is run again in the background on
  `zen-db`'s runtime and its rows go through a bounded queue (16 pages of 2 000 rows) to a writer
  thread, which formats and writes them, so memory stays flat (a million rows grow the process by a
  few MiB). PostgreSQL reads through its portal page by page; SQLite's `sqlite3` output is now read
  as it is printed (`DriverSession::stream` for SQLite); MySQL streams its text-protocol rows. On
  PostgreSQL, CSV and TSV go through **`COPY (query) TO STDOUT WITH (FORMAT csv …)`**
  (`DriverSession::copy_out`) whenever the settings fit (one-character ASCII delimiter and quote,
  NULL text without them; setting `serverCopy`); our rewriter adds the formula escapes to text
  columns and CRLF as the bytes stream. COPY writes PostgreSQL's text forms (booleans `t`/`f`,
  `timestamptz` in the session's zone).
- **Sessions**: a result is read again in its own context (its filter and sort kept; its open portal
  closed first); a console's query in the console's session (it sees the open transaction); a
  query or tables in a **read-only, auto-commit session of their own**, closed at the end.
- **Files** are written beside the target under a hidden temporary name and moved into place only
  when complete; cancel or a failure removes it, so an existing file is never half replaced. A file
  that exists is refused (`exists`) until the person agrees to replace it. Names come from a
  pattern (`${table}`, `${connection}`, `${timestamp}`, `${date}`, `${index}`), with characters a
  file name cannot hold replaced, so a name never leaves the folder. The clipboard takes at most
  64 MiB.
- **Progress and end**: the dialog shows the item, rows, bytes and time, with Cancel Export (the
  server's cancel) and Run in Background; the end is a notification with **Open** and **Reveal in
  Finder**. The format, its settings, the folder, the pattern and the destination are kept in
  `<data folder>/databases/export.json`.
- **Parquet** (ADR-0181): `zen_db::parquet_writer` types each column from its type
  name through the portable `SqlType` (`Decimal128` for exact numbers with a known scale,
  microsecond timestamps, UTC for those with a zone, UTF-8 for everything without a portable form),
  gathers rows into Arrow arrays 4 096 at a time and writes row groups (`rowGroup`, 65 536 by
  default) with Snappy, gzip (flate2's Rust backend), LZ4 or none: only pure-Rust codecs, no Zstandard or
  Brotli. A value its column cannot hold fails the item with the column's name. Binary, so never to
  the clipboard (`binary`); the dialog shows Compression and Rows per row group instead of the text
  settings.
- **Not yet**: XLSX, compression of text formats, encodings other than UTF-8,
  several result sets of one statement; ClickHouse's streamed body is not yet bounded by the queue
  (its reader thread reads ahead); a PostgreSQL table of another database than the connection's
  (with Show all databases) is read in the connection's database.

## Backup and restore

Backup and restore for PostgreSQL and MySQL run the databases' own client tools, which the IDE
finds itself: no "local client" folder to set.

- **Finding the tools** (`zen_db::backup::find`): `pg_dump`, `pg_restore`, `psql`, `mysqldump` /
  `mariadb-dump`, `mysql` / `mariadb` (MariaDB's names first for a MariaDB connection) are looked for
  on `PATH`, in Homebrew's `bin` and the `bin` of its keg-only formulae (`libpq`, `postgresql@N`,
  `mysql-client[@N]`, `mysql`, `mariadb`; the newest version first) under `/opt/homebrew`,
  `/usr/local` and Linuxbrew, in Postgres.app's versions, then `/usr/bin` and `/bin`; each program
  once (by its real path), with its `--version` read. For a server on this computer (`localhost`,
  `127.0.0.1`, `::1`, `host.docker.internal`), `docker ps` finds the running container that publishes
  the connection's port and `docker exec … sh -c 'command -v …'` lists its tools and versions. Every
  probe has a 5-second limit and runs on a thread of its own (`db.backup.tools` answers `searching`
  until done; the dialog polls). `choose` takes the newest local tool, or the container's when it is
  newer (pg_dump refuses a newer server).
- **Command lines** (`zen_db::backup::{backup, restore}`): the connection's host, port, user and
  database (`--no-password`, MySQL `--protocol=TCP`), its TLS mode (`PGSSLMODE` and the certificate
  files for libpq; `--ssl-mode` for MySQL's client, `--ssl`/`--skip-ssl` for MariaDB's) and the
  options. In a container the tool runs as `docker exec [-i] [-e PGPASSFILE=<copy>] <id> <tool>` against
  `127.0.0.1` and the container's own port; its output or input is redirected to or from the host's
  file through `/bin/sh -c 'exec "$@" > "$ZEN_BACKUP_OUT"'` (the path never parsed by the shell), as
  is `mysql`'s input everywhere. The directory format needs a local tool. MySQL 8's `mysqldump`
  against MariaDB gets `--column-statistics=0`. A PostgreSQL backup's format is read from its first
  bytes (`PGDMP`, `ustar`, a folder): plain SQL goes through `psql -v ON_ERROR_STOP=1`, the others
  through `pg_restore` (`--create` connects to `postgres`).
- **Passwords** (ADR-0193) are read on the run's thread (the saved one through the window's secret
  store, or `password` for this run) and written into a new file only the user can read (0600,
  `create_new`, never a link), in the tool's format: a `.pgpass` line named by `PGPASSFILE`, or a
  `[client]` option file given first as `--defaults-extra-file=`. For `docker exec` the file is
  copied into the container (`docker cp`) and removed there afterwards. The file goes when the run
  ends, fails, is stopped or killed. Never `PGPASSWORD`/`MYSQL_PWD`, never an argument; the run's
  stored command line, console, configuration and answers never hold it.
- **Versions** (PostgreSQL): before a job the server is asked its version with `psql`
  (`SHOW server_version_num`, the password in the same kind of file). A tool of the server's major
  version is taken when one is found (a Homebrew `postgresql@NN`, Postgres.app, the container's);
  else `pg_dump` must not be older than the server and `pg_restore` not newer — otherwise the job is
  refused before anything runs, naming both versions and `brew install postgresql@<server>`. A
  plain script runs with any `psql`. An unknown version keeps the tool chosen.
- **Names and agents**: the database goes to libpq as `--dbname=dbname='…'` (a name with `=` or
  `postgres://` is never read as a connection string); a MySQL database or table name starting with
  `-` is refused (`badName`). Agents may not pass `extraArgs` (`agentExtraArgs`) and never change the
  remembered settings; a backup file or restored file hidden from agents is refused (`redacted`).
  `PATH` folders that are relative or inside the open project are never searched for tools.
- **Runs** (`zen_workbench::database::backups`): each backup or restore is a run of kind
  `databaseBackup` added with `Runs::insert_task`, so the Run tool window shows it with its console
  and ■ stops it (the program's group is interrupted, then ended). A backup writes
  `.<name>.<pid>.part` beside the target and renames it into place after exit code 0; failure, stop
  or the window closing removes it. A restore always asks first (`confirmExecute` with a token over
  the connection, database, file and options); read-only connections refuse; agents need full
  access for a restore and any access for a backup. The folder and options are kept in
  `<data folder>/databases/backup.json`.
- **The dialog** (`ui::ide::backup_dialog`, Tools ▸ Backup…/Restore… on a connection, Backup on a
  schema or table of PostgreSQL, MySQL and MariaDB connections): the tool (Automatic or one found),
  the database, the tool's options, the folder and file name (or the backup file), the question
  before replacing a file or restoring, a password field after `passwordNeeded`.
- **Rerun** in the Run tool window (`run.process.rerun`, `backups::rerun`) starts the job again from
  the parameters kept in the run's configuration (never the password or a confirmation) with the
  tool it used; a backup replaces its file. A restore is always asked again, and a password typed for
  one run is asked again: the refusal is answered and left in the session (`backups::take_asked`),
  and the window opens the dialog with the job's settings and the question or the password field.
- **Not yet:** connections through SSH (refused with `sshTunnel`); SQLite and the other drivers;
  `/bin/sh` redirection, so a
  container's tool and `mysql` restores need macOS or Linux; a restore cannot be undone half way
  unless it ran in one transaction.
## Dialog layout and safe defaults

- **Shared rows** (`ui::ide::form`): every database dialog puts its labels in one left-aligned column
  of a fixed width (they extend, never cut, in a longer language), and draws egui's checkboxes,
  radios and combo boxes with the theme's field colours (`IdePalette::text_field`,
  `text_field_border`, the accent when hovered or open), so they are framed and visible in both
  themes. Long pages (the connection dialog's SSH/SSL) show their scroll bar while the content
  overflows (`form::solid_scroll_bars`), not only while scrolling.
- **Connection dialog**: **Connect after saving** (new connections, on by default) runs
  `db.connection.connect` after `db.dataSource.add` or `db.detect.accept`, with the password typed;
  the password-needed error is written for people (`database-error-password-needed`); agents still
  get the code `passwordNeeded`.
- **Transfer**: connections whose own or type's `readOnly` is set are marked in the target list and
  never preselected; with no other target the dialog starts without one and asks for it. Remembered
  load settings other than the defaults are named as such, with **Reset to defaults**. The steps
  share one body height, so the dialog keeps its size. The finished notification uses plurals for
  rows and tables.
- **Export**: `db.export.settings { source }` names the source's table and connection and tells
  whether the server can `COPY`; the dialog shows the COPY option only then. With a grid selection
  of more than one cell, **Selected cells only** exports `{ kind: cells, id, rows, columns }`: the
  result's loaded cells (staged edits included) rendered by Copy As's writer on a copy job's thread
  and written to the file there (at most 100 000 rows; never Parquet), announced like an export.
  **Browse…** picks the folder; the last folder is remembered.
- **CSV import**: **Browse…** picks the file; the sample's headers never wrap; truncating has its
  own **Before loading** row, repeated on Confirm. Transfer's Loading and Confirm steps do the same.
- **Bind Parameters**: a value is checked only once typed (an empty one only keeps Run off); the
  type selectors are framed and the rows aligned.
- **Confirmations**: the dangerous-statement dialog offers **Skip** only when the run has several
  statements; the staged-edits question asks one question for people (closing a tab: "Save them
  before closing it?") while agents keep the `pendingChanges` message with the counts.

## Import from CSV

- **One dialog in five steps** (`ui::ide::csv_import_dialog`, one modal): File → Format (delimiter, header, quote, escape, NULL text, date format, a sample
  of 100 rows re-read as the format changes) → Columns (target column or Skip, how each is read; names
  and types for a new table) → Loading (rows a statement, commit every N, truncate first, stop or
  skip, COPY) → Confirm. **Import Data…** on a table, a schema or its Tables folder; window action
  `view.database.import`. Commands `db.csvImport.settings`, `.preview`, `.start`, `.state`,
  `.cancel` ([automation.md](automation.md#import-from-csv)).
- **Reading** (`zen_db::csv_import`): a reader that streams records from a `BufRead`, one line at a
  time, quoted fields across lines, doubled quotes or an escape character, a UTF-8 BOM, blank lines
  passed over, invalid UTF-8 replaced; each record keeps the line it starts on. An unquoted field
  equal to the NULL text is `NULL`, a quoted empty one is empty text. Kinds are guessed from the
  sample (integer, decimal, boolean words, ISO or patterned dates and timestamps, else text) and
  read from a column's type name for every driver (`Kind::of_type`). Date patterns are Java style
  (converted) or strftime (`jiff`); dates and timestamps read with a pattern are sent as ISO 8601.
- **Loading** (`Engine::import_csv`): the file is read and converted on a thread of its own and sent
  in batches through a queue of two, so memory holds a few batches. The import runs in a session of
  its own (SQLite's shared one, refused while a transaction is open there), in auto-commit with its
  own `BEGIN`/`COMMIT`: one transaction for the file, or a commit every N rows. `CREATE TABLE` and
  `TRUNCATE TABLE` (SQLite: `DELETE FROM`) run first in that transaction, so a stopped import rolls
  them back on PostgreSQL and SQLite (MySQL commits DDL at once). Each batch runs inside
  `SAVEPOINT zen_import`; when it fails, it is rolled back to the savepoint and retried row by row to
  find the failing rows, each noted with its row, line and the server's code and detail (at most
  1 000 kept). PostgreSQL sends a batch through `COPY table (columns) FROM STDIN WITH (FORMAT csv)`
  (`DriverSession::copy_in`, rows re-encoded so NULL and empty text differ); other drivers use
  multi-row `INSERT`s written with `SqlDialect` literals (ADR-0170); ClickHouse, without
  transactions, inserts batch by batch. A transaction pooler refuses `BEGIN`, and the import then
  runs without one. Cancel stops the running statement on the server and rolls back what is not
  committed. The generated statements are in the query history with the source **Import**.
- **Safety** (ADR-0172): `db.csvImport.start` passes the `CREATE`/`TRUNCATE` and the `INSERT` head
  through `safety::guard` before connecting, so read-only connections refuse (`readOnly`), types
  that confirm SQL execution and `TRUNCATE` ask (`confirmExecute`), and agents follow the type's
  access; the engine refuses a read-only connection again. Destructive steps touch only the target
  table: `CREATE TABLE` (never `IF NOT EXISTS` or `DROP`; refused when the snapshot knows the table)
  and the truncate are explicit choices.
- **After an import** that committed rows, the open object tab of its table (same connection and
  tree path) reads its Data page again, unless it has staged changes.
- **Not yet**: encodings other than UTF-8, other source formats (JSON, XLSX), several files at once,
  mapping by expressions or
  constants, and `COPY`'s own error position is not used (the row-by-row retry finds the row).

## Data transfer

- **One dialog in four steps** (`ui::ide::transfer_dialog`, a database as the target): Target (the tables, the target connection and schema) → Tables and Columns
  (target names, existing or new, columns renamed, left out or retyped) → Loading → Confirm, then
  progress with Cancel Transfer and Run in Background. **Transfer Data…** on a table, a schema or
  its Tables folder; window action `view.database.transfer`. Commands `db.transfer.preview`,
  `.start`, `.state`, `.cancel` ([automation.md](automation.md#data-transfer)).
- **Plan** (`zen_workbench::database::transfers`, from both snapshots, never connecting): an
  existing target table takes the source columns of the same name (case-insensitive, generated
  columns left out); a new one every column with its type: the source's text between the same
  dialect (and MySQL ↔ MariaDB), else `zen_db::transfer::SqlType` read from any driver's type name
  and written in the target's dialect (ClickHouse's `Nullable(…)` and `MergeTree ORDER BY` the key).
  The source's primary key is kept when all its columns are copied. Columns not yet read are read
  in the background (`loading`; ask again).
- **Copy** (`Engine::transfer`): tables one after another; each is read with
  `SELECT columns FROM table` in a read-only, auto-commit session of its own, page by page through
  the driver's stream, and the pages go through the CSV import's bounded queue into its loader
  (`load_rows`): `CREATE TABLE`/`TRUNCATE` first in the table's transaction, savepoints per batch
  with the row-by-row retry, `COPY … FROM STDIN` on PostgreSQL, else multi-row `INSERT`s. Values are
  converted for the target column's portable type (`transfer::cell`): booleans, numbers, timestamps
  with an offset written in UTC where the target has no zone, bytes as `\x…` on PostgreSQL, arrays and
  documents as JSON; a value that cannot be is a row error (`notABoolean`, `notANumber`,
  `notADate`). SQLite's shared session cannot read and write itself (`sameSession`). Cancel stops
  the read and the write and rolls back what is not committed; the tables after it are not started.
  Statements are in the query history with the source **Transfer**.
- **Safety** (ADR-0172, ADR-0179): `db.transfer.start` passes the `CREATE`/`TRUNCATE` and the
  `INSERT` heads through the target's `safety::guard` and the `SELECT`s through the source's agent
  access; agents are refused a transfer naming a connection closed to them on either side, and
  `db.transfer.state` leaves such transfers out. A table is created only when the target's snapshot
  does not have it.
- **Not yet**: queries as a source, column expressions and constants, dropping or recreating
  target tables, upserts, several tables at once, Zstandard in Parquet.

## Find in Database

- **What**: full-text search of the data and search of the names in one tab (`database_view/find.rs`, `zen_workbench::database::find`):
  a text, a connection and a schema; data and/or names; match case; column kinds (text by
  default, numbers, dates and times, other), only some columns by name, views on request; rows and
  seconds per table. Opened from the toolbar's magnifier and **Find in Database…** on a connection
  or schema in the navigator. Commands `db.search.targets`, `.open`, `.start`, `.state`, `.cancel`,
  `.openResult` ([automation.md](automation.md#find-in-database)). Name search over the snapshot
  stays with Search Everywhere's Database tab; this is the server's search.
- **SQL** (`zen_db::search`, in each driver's dialect, ADR-0170): the catalogue is read from the
  server (`pg_class`/`pg_attribute` without partitions and foreign tables; `information_schema` on
  MySQL and MariaDB; `sqlite_master` with `pragma_table_info`; ClickHouse's `system.columns`
  without engines that reach other systems, such as Kafka, URL, S3 or MySQL), so tables created after
  the last refresh and schemas read by names only are searched too. Each table is read with
  `SELECT CASE WHEN c₁ … THEN 1 ELSE 0 END, … FROM t WHERE c₁ … OR c₂ … LIMIT n+1`: only flags come
  back, never values. Case-insensitive: `ILIKE` (PostgreSQL, ClickHouse), `LOWER(CAST(… AS CHAR
  CHARACTER SET utf8mb4))` (MySQL, MariaDB), SQLite's `LIKE` (ASCII letters only); case-sensitive:
  `LIKE`, `COLLATE utf8mb4_bin`, `instr()`. Patterns escape `%`, `_` and `!` (`ESCAPE '!'`;
  backslashes on ClickHouse); MySQL text with a backslash is a hex literal.
- **Engine** (`Engine::search`, `engine/search.rs`): in the background on `zen-db`'s runtime, in a
  read-only, auto-commit context of its own (`search-<id>`; the shared session on SQLite, locked one
  statement at a time), one table after another: each with its deadline (`stream_guarded`, the
  server's cancel; a session that does not confirm is reopened for the next table) and its limit;
  the IDE's own transaction of a PostgreSQL portal is rolled back after each table, so no lock
  outlives it (ADR-0168). Progress, counts per column, timeouts and errors per table; Cancel stops
  the table being read. A lost connection ends the search; other errors mark the table. Every
  statement goes into the query history with source `search`.
- **Opening** (`db.search.openResult`): the object tab on Data with the WHERE filter
  `c₁ LIKE … OR c₂ …` set before it reads (`filters::filter_tab`: Back returns, the condition goes
  into the table's filter history, staged edits refuse it); a table found only by name opens
  unfiltered; a table missing from the snapshot or a driver with its own tree (ClickHouse) opens a
  console running the filtered `SELECT`.
- **Load**: warnings for a type that confirms SQL execution (`production`), a schema over a million
  rows by the snapshot's estimates (`largeSchema`) and views; one search per window (a new one
  cancels the running one). Agents follow ADR-0172 (`none` refuses; reads are allowed elsewhere).
- **Not yet**: one schema per search (several later); whole-word and regular expression
  matching; searching `LOB`s by size; a full-text index (`to_tsvector`, `MATCH`) where one exists;
  Redis and other key-value drivers.

## Schema and data compare

- **Commands** `db.compare.schemas`, `.data`, `.state`, `.cancel`, `.ddl`, `.script`, `.open`,
  `.saveSnapshot`, `.snapshots` ([automation.md](automation.md#schema-and-data-compare)); the
  **Compare** tab (`ui::ide::database_view::compare`, the toolbar's Compare button) is drawn from
  them. It follows the window's latest comparison, whoever started it: on each new one
  (`Compares::revision`) it reads `db.compare.state` and takes its sides from `request`; a data
  comparison of other tables than the shown schemas replaces the schema view with its rows.
- **Sides**: a connection's schema, read again first in the Metadata context (only what changed,
  details up to 20 000 objects; `refresh: false` compares the snapshot kept, offline, or the newest
  read of an earlier comparison), or a **saved snapshot**: one schema's structure as JSON in
  `<data folder>/databases/snapshots/` (never data values or secrets). Reading runs on a thread of
  its own and through the engine's runtime; Cancel stops the introspection.
- **Comparison** (`zen_db::compare`, pure, compiles for WebAssembly): objects pair by kind and key
  (a routine's with its arguments), then case-insensitively; columns by name (type, nullability,
  default, identity, generation, collation, comment), keys by name and then by what they cover (so a
  primary key named differently is the same key), indexes that enforce no key likewise, triggers,
  policies, view and routine definitions, sequences, types, comments, row security, partitioning.
  Definitions compare with whitespace collapsed and each side's own schema qualifier removed, so
  `app.orders` and `public.orders` match. Objects whose details were not read compare by name only
  (`partial`). Column order is not compared.
- **DDL side by side**: each side's driver generates the object's DDL offline (with indexes,
  triggers and comments) and the IDE's diff viewer shows it (no syntax colours yet: the workspace
  view has no highlighter).
- **Sync script** (`zen_db::compare::sync_script`): text in the target's dialect: drops of changed or
  removed foreign keys and parts first, removed objects (only with `drop`, else `--` notes), new
  types, sequences and tables (the target driver's DDL of the source object moved into the target
  schema, foreign keys held back), column changes (PostgreSQL `ALTER COLUMN … TYPE/SET NOT NULL/SET
  DEFAULT/identity`, MySQL and MariaDB `MODIFY COLUMN`, ClickHouse `MODIFY COLUMN`; SQLite cannot, a
  note), keys, indexes, triggers and policies, views and routines (`CREATE OR REPLACE` on
  PostgreSQL, drop and create elsewhere), foreign keys last, comments and row security. MySQL,
  MariaDB and ClickHouse scripts start with `USE` of the target, as their DDL names objects
  unqualified. The statements are written in a module of their own with the dialect helpers of
  ADR-0170, to move onto the shared `zen_db::alter` generator when it lands.
- **Data compare** (`Engine::compare_data`, `zen_db::compare::data`): the source is read page by
  page (`ORDER BY key LIMIT n`, then `WHERE key > last`), each page looked up on the target by key
  (`IN`, or `OR` of `AND`s for composite keys), then the target is read the same way for the rows the
  source lacks. Each page is a statement of its own in a read-only, auto-commit session of the
  comparison's own (closed at the end), so no lock outlives a page (ADR-0168). Values compare by
  meaning across servers (`5` = `5.0`, `true` = `1`). Counts are exact; the differing rows kept are
  capped (10 000), and the data script (`INSERT`, `UPDATE` of the differing columns, `DELETE` only
  with `drop`) covers those.
- **Safety**: nothing runs a script; **Open Script in Console** creates a console of the target's
  connection, where ADR-0172's confirmations and agents' rules apply. Agents' `none` access refuses
  comparisons of that connection.
- **Tests**: `zen_db::compare` unit tests; `tests/compare.rs` runs both comparisons and applies both
  scripts against the real `sqlite3`, and (ignored, Docker) PostgreSQL 18 and MySQL 8.4; the
  workbench and the tab (light and dark renders) have their own.
- **Not yet**: column order and renames (a renamed column is a drop and an add), cross-dialect
  scripts beyond names and types, SQLite table rebuilds, comparing several tables' data at once,
  data compare of saved snapshots (they hold no rows).

## Production safety

**Connections closed to agents** (ADR-0179). `database::agent_access` registers every `db.*` command
in `ClosedToAgents`, except `OPEN_TO_AGENTS` (the commands about the connection itself). For an agent,
the wrapper refuses a call that names a connection whose type gives agents `none` (`connection`,
`connections`, a console's `path`, a `tab`, a result `id`, `source`/`target`, else what is in front)
with `agentNotAllowed`, and scrubs the answer: list items that refer to one are dropped, and any other
part refuses the whole answer. `workspaces::AgentHiddenTabs` leaves such tabs out of
`workspace.host.state`. `hidden_files::AgentHiddenPaths` adds the consoles and snapshots of those
connections, plus, while any connection is closed, the cached structure, the history, the crash
journals, the filters and the navigator's state, to the files hidden from agents.
`db.compare.snapshots` and snapshot sides check the snapshot's own connection.

([ADR-0172](adr/0172-production-safety-and-agents-on-databases.md).)

- **Classification** (`zen_db::safety`): `strip_in` of the dialect's lexer (comments out, strings as
  `''`, quoted identifiers kept), then tokens. The first word decides the effect (`read`, `session`,
  `transaction`, `data`, `structure`, `other`); a read with a data verb after `(`, `)` or
  `EXPLAIN ANALYZE`, `SELECT … INTO` or a call of a function with side effects is `data`; a session
  or transaction statement that names `write`, `read_only`, `query_only`, `role` and the like is
  `other`; MySQL's executable comments `/*!…*/` are `other`. Risks: `DELETE`/`UPDATE` at a verb
  position (not after `ON`, `DO`, `FOR`, `KEY`, a comma…) without a `WHERE` at their own depth, or
  one whose top-level `OR` has an always-true part; a MySQL join's `ON` counts as a filter,
  PostgreSQL's `USING` does not; `DROP` (also `ALTER … DROP`, not `DROP DEFAULT`/`NOT NULL`),
  `TRUNCATE`; ClickHouse mutations (`ALTER TABLE … DELETE WHERE 1`) are judged too. One-line
  languages: `Driver::classify` says read or not, `FLUSHALL`/`FLUSHDB` are risks.
- **Engine**: `QueryOptions::read_only` and a read-only connection (its own flag or its type's
  `readOnly`) make the context's session read-only on the server before the first statement, and
  every statement that is not allowed read-only fails with kind `refused`, code `readOnly`, before it
  is sent, recorded in the history; a driver that asks the server which commands write
  (`Driver::guards_read_only`: Redis) judges a person's commands itself. `Engine::set_read_only` applies the lock to open sessions;
  `save_edits_from` refuses on a read-only connection. A session leaves read-only when nothing asks
  for it any more.
- **Workbench** (`zen_workbench::database::safety::guard`): called by every console run, `db.query.run`
  and `db.result.refresh`, before connecting: agents' `none` → `agentNotAllowed`; agents' statements
  that `zen_db::safety::reaches_outside` finds on a `sql:duckdb` connection → `agentOutsideDatabase`
  with `handOff` (`agent_reads` too); read-only →
  `readOnly`; agents' `readOnly` → `agentReadOnly` with `handOff`; then confirmations →
  `confirmExecute` with a token (a hash of the connection and the statements' texts) and `retry`.
  `agent_changes` guards saves, Commit and key changes; `agent_reads` filters and object reads;
  `agent_loosens_source`/`_type` connection and type changes.
- **Window**: `execute_dialog.rs` (a transaction dialog, `executeConfirmation`): one statement at a
  time, Yes / Skip / Yes to All / Cancel, Run Anyway after the risk's checkbox, no Yes to All past a
  risk, Enter consumed, window shortcuts off while open, a second refusal for the same token keeps
  the progress. The console toolbar has the type badge and the lock (`db.connection.readOnly`); a
  console of a type that confirms SQL execution is framed in the type's colour
  (`TextExtras::frame`).
- **Not yet:** asking about `ALTER` on large tables (no size check); per-statement confirmation
  inside a running script (asked up front instead, ADR-0172); the connection dialog and Settings →
  Connection Types do not yet show `agents` and the type's `readOnly` (commands only); an agent can
  still add a second connection of a looser type to the same server (it never learns the password).

## Explain Plan

- **Model** (`zen_db::explain`, also for WebAssembly): `ExplainOptions` (`analyze`, `verbose`,
  `costs` on by default, `buffers`, `settings`), and each driver's statement through
  `Driver::explain` and `Driver::explain_options`: PostgreSQL `EXPLAIN (FORMAT JSON, …)` with every
  option (`BUFFERS FALSE` with ANALYZE, as PostgreSQL 18 turns buffers on by itself); MySQL
  `EXPLAIN FORMAT=JSON`, and for ANALYZE `EXPLAIN ANALYZE` (its tree text: MySQL 8.4 has no JSON
  for it); MariaDB `EXPLAIN FORMAT=JSON` and `ANALYZE FORMAT=JSON`; SQLite `EXPLAIN QUERY PLAN`.
  Options a driver lacks are listed in `ignored`. `explain::parse` reads each answer into one tree
  of `PlanNode`s in tree order (node, entity, condition, startup cost, cost, self cost, estimated and
  actual rows, loops, time over all loops, self time, every property as `details`). Shares are of
  the self time when analyzed, else of the self cost (MySQL: read plus eval cost of a table), and a
  node of 20% or more is `hot`.
- **Engine** (`Engine::explain`, operation kind `explain`): in the context's own session (a console's),
  with its schema and read-only state; a read-only session refuses an `EXPLAIN ANALYZE` of a change
  before it is sent. ANALYZE runs after `START TRANSACTION` and is rolled back, or between
  `SAVEPOINT zen_explain` and `ROLLBACK TO SAVEPOINT` inside an open transaction; a failed rollback
  closes the session. The answer is read into a plan on the blocking pool; `take_plan` hands it over.
  Every explain is in the query history; cancel with `db.connection.cancel { operation }`.
- **Workbench** (`zen_workbench::database::plans`): `db.console.explain` takes the selection or the
  statement under the caret (or `statement`, or a Plan tab's statement) and is guarded as
  `EXPLAIN <statement>` or `EXPLAIN ANALYZE <statement>` (ADR-0172): a plain explain is a read, an
  ANALYZE of a change asks or is refused exactly as running it. One Plan tab per console
  (`plan:<context>`), kept in `workspace.json` with the connection, statement (4000 characters) and
  options, never the plan; after a restart it is `notRun` and connects at the next Explain.
  `db.plan.state` answers 100 nodes by default (`limit` up to 1000, `truncated`), details only of the
  chosen node unless asked, and the raw text only with `raw` (20 000 characters).
- **Window** (`database_view/plan.rs`): a bar with Explain, Cancel, the driver's options, Tree and
  Text, and the totals; a warning strip while ANALYZE is ticked; the tree virtualised by rows, hot
  nodes tinted from the theme's warning and error colours by their share; the details pane from
  820 px wide. The console toolbar has Explain Plan (⌥⌘E, context Database).
- **Not yet:** collapsing subtrees, comparing two plans, plans of ClickHouse (`EXPLAIN`), and a
  graphical plan diagram.

## Copy and paste

- **Copy** (`ui::ide::database_view::clipboard`, `workbench::database::clipboard`, formats in
  `zen_db::clip`): ⌘C reaches a focused grid as egui's copy event (winit turns ⌘C into it, so the
  grid's old `Key::C` never saw it) and copies plain values: tab-separated, one row a line, quoted
  only where a value holds a delimiter, the quote or a line break, NULL as nothing and empty text as
  `""`, so a copy pastes back exactly. Copy with Header, Copy Column Names, Copy Row as JSON (whole
  rows) and **Copy As ▸**: CSV, TSV, JSON, Markdown, HTML, XML and SQL `INSERT` through
  `zen_db::export::Writer` (a header, no formula escapes, the result's table), SQL `UPDATE` through
  `edit::generate` (needs the rows' table and key), SQL `IN (…)` (one column, or row values for
  several, repeats dropped), and literals for Go (a slice of maps), Rust (a `Row` struct whose field
  types come from the values, `Option` where one is NULL), TypeScript, Python and Kotlin. Values are
  the staged ones; SQL is in the connection's dialect.
- **Advanced Copy** (⇧⌘C): column names, row numbers, column and row delimiters, quote, quoting
  (needed, always, never), NULL text and value style (as shown, SQL literals, JSON), kept in
  `<data folder>/databases/copy.json` with the last Copy As format and paste's NULL text.
- **Off the window's thread**: the window copies with `clipboard: true`; a selection over 50 000 cells
  is written by a thread of its own page by page (`db.result.copyState` follows and cancels it; the
  bar under the grid shows Cancel Copy), its text put on the clipboard when done. A copy stops at
  100 000 rows or 64 MiB and says so; agents' answers stop at `limit` characters (100 000).
- **Paste** (⌘V, Paste as New Rows, ⌥⇧⌘V Advanced Paste): `zen_db::clip::parse` reads TSV and CSV as
  spreadsheets copy them (a BOM, LF, CRLF and CR, quoted fields with delimiters, doubled quotes and
  line breaks, one trailing line break dropped); the delimiter is a tab when there is one, else the
  one of `,`, `;`, `|` every line has equally many of; a first line naming the columns is skipped.
  Every value goes through `edit::parse_input` for its column's type; read-only and missing columns,
  NULL in NOT NULL columns and deleted rows are problems with the line and column, never the value.
  With problems nothing is staged and the window opens Advanced Paste with a preview of the rows and
  the problems, and **Paste Valid Cells**. Cells are staged over the rows from the anchor and rows
  past the last are added as new rows, at most 100 000 cells; nothing reaches the
  server before Save.
- **A fix on the way**: a press on a menu or a dialog over a grid no longer selects the grid's cell
  under it (before, Delete Rows chosen in a menu over another row could act on that row).
- **Not yet**: Copy As offers every language rather than the project's ones first; "copy as source
  code" templates and the rich-text (HTML) clipboard flavour; pasting images or files.

## Bind parameters and script variables

- **Placeholders** (`zen_db::params`): a byte scanner per dialect marks what is code (strings with
  doubled quotes, MySQL backslashes, PostgreSQL `E''`, double-quoted and backticked names, `[names]`
  on SQL Server, `--`, `#` and nested `/* */` comments, `$tag$` quotes) and finds `:name` (not after
  a word, `:`, `$` or `@`, so `::type`, `a:b`, `[1:n]` and `:=` are not placeholders), `$n` in
  PostgreSQL and standard SQL (SQLite, DuckDB), `?` and `?n` outside PostgreSQL (its JSON
  operators); a `?` after `?n` is `?n+1`, as SQLite numbers it. `bind` rewrites them for the driver:
  PostgreSQL numbers distinct names `$1…` (one name, one parameter), MySQL and MariaDB get one `?` per
  occurrence; a driver without `bind_parameters` (SQLite through `sqlite3`, ClickHouse over HTTP)
  gets `SqlDialect::literal`s instead (ADR-0170), told in the output as `parametersInlined`.
- **Values** are typed (`ParamValue`: text, number, boolean, NULL): text goes as text the server
  converts; numbers as `Integer` or exact `Decimal`; MySQL's prepared statements get typed values
  (`Int`, `Double`, bytes), so `LIMIT ?` works.
- **Engine**: `QueryStatement::params` (named values in the driver's order) go through
  `DriverSession::stream_bound`, which PostgreSQL implements on its prepared statement and portal
  paging (unnamed `Bind` with text values) and MySQL with `exec_iter`; the default is `stream` when
  there are no values and `Unsupported` otherwise. A result keeps its values
  (`ResultSet::statement_params`), so Fetch Next past the cap, Count, Refresh and filters read it
  again with them. `QueryOptions::notes` seed the query's output.
- **Kept text** (ADR-0178): `QueryStatement::kept` is the statement as written — placeholders as
  written, secret-looking `${name}` masked — when it differs from the text sent; the history, the
  query state's summaries and the confirmations show it (`QueryStatement::shown`), so values written
  as literals for SQLite and ClickHouse never reach them. ClickHouse statements are read with
  backslash escapes and backtick names (the MySQL lexer) when placeholders are looked for.
- **History**: `Ran::params`; an entry keeps `parameters` (names; `:name = value` with the setting
  `parameterValues`, off by default), secret-looking names masked, the line passed
  through the password masking; the setting is taken when the statement is recorded.
- **Safety**: the confirmation lists each statement's parameters, and the token hashes them with the
  texts, so a token confirmed for some values does not run others.
- **Variables** (`workbench::database::variables`): before a run, client statements `@set name =
  value` (now client commands of every SQL dialect's splitter, like psql's `\` lines) set variables
  in order, `@include file.sql` is split and inlined (next to a script, else in the project; depth
  8, cycles refused), `${name}` is expanded everywhere (strings too: it is text). Missing variables
  or values, or placeholders without `parameters` given, refuse with `parametersNeeded` and `retry`;
  the window's `parameters_dialog.rs` (a transaction dialog, `bindParameters`) asks them and runs
  `retry` with `parameters` and `variables`, chaining into the confirmation when that asks next.
  Values and variables are remembered per console path in `<data folder>/databases/variables.json`,
  secret-looking names only in memory. The results panel's Variables tab
  (`database_view/variables.rs`) edits them through `db.console.setVariable` and
  `db.console.setParameter`.
- **Default types** (`params::infer_types`): a token pass over the code parts of a statement gives
  a placeholder `Number` next to a numeric literal, in arithmetic, after `LIMIT`/`OFFSET`/`TOP`/
  `FETCH FIRST`, or when compared (`=`, `<>`, `<`…, `IN (…)`, `BETWEEN … AND …`) with or assigned to
  a column whose type `params::column_type` reads as numeric; `Boolean` next to `true`/`false` or a
  boolean column; a cast's type (`::int`, `CAST(… AS …)`); nothing otherwise (`Text`). Columns come
  from the connection's cached snapshot (`structure::ensure`, read from disk once, never connecting),
  only from tables the statement names, and a column name with different types in them tells
  nothing. The refusal and `db.console.parameters` answer it as `inferred`; the dialog starts with
  it unless a value is remembered. The dialog has no date type, so dates stay `Text`.
- **Explain and several connections** take the same step. `db.console.explain` prepares its one
  statement with `variables::prepare` (its own `retry`), sends the driver's explain of the rewritten
  text and `ExplainRun::params`, which the engine streams through `stream_bound` into a collecting
  sink when there are values (PostgreSQL's unnamed prepared statement, MySQL's `exec_iter`);
  `inlined` lists the values a driver without binding gets as literals, shown in the Plan tab.
  `db.script.runOnConnections` prepares every connection in its own dialect first, merges their
  `parametersNeeded` refusals into one (each name once, `inlined` if any connection inlines), and
  starts each connection with its notes; a target answers its `inlined`. After the Bind Parameters
  dialog, a confirmation listing connections reopens the several-connections dialog.
- **Not yet**: ClickHouse's own `{name:Type}` parameters (values are inlined there); PostgreSQL behind
  a transaction pooler refuses bound values (no prepared statements); an error position the server
  reports is in the rewritten text, so a long `:name` shifts the mark a little; `@set` from a query's
  result (`@set x = select …`) is not evaluated, the text after `=` is the value.

## Sessions and locks

- **Drivers** (`zen_db::sessions`, `DriverSession::sessions`, `::end_session`, `::server_id`;
  `Capabilities::sessions` is true for PostgreSQL, MySQL and MariaDB, now false for ClickHouse, which
  has no implementation yet). PostgreSQL reads `pg_stat_activity` (client backends and autovacuum
  workers; `state <> 'idle'` without Show idle, so idle-in-transaction stays) with
  `pg_blocking_pids`, and in a second query the locks not granted plus the granted locks on the same
  target held by a blocker (`pg_locks`, at most 500); both through the simple protocol, so a pooler
  works. MySQL reads `information_schema.PROCESSLIST` and `performance_schema.data_lock_waits` /
  `data_locks`; MariaDB `INNODB_LOCK_WAITS` / `INNODB_LOCKS` / `INNODB_TRX`. Without Show idle,
  `Sleep` connections are left out unless they block someone. A missing privilege for the locks
  leaves the sessions and says why (`locksError`).
- **Engine** (`engine/sessions.rs`): `Engine::sessions` and `Engine::end_session` run in the
  Metadata context in the background, waiting for a connection that is connecting and never
  connecting one; the listing is finished in `zen_db::sessions::finish` (who blocks whom, the
  blocking chains from each waiting leaf to its root, cycles cut where they repeat, queries cut and
  masked by `history::redact`) and this IDE's sessions are marked from each context's
  `server_id`. PostgreSQL reads `pg_backend_pid()` in the same round trip as the session guards.
  `end_session` builds its statement by `end_statement` (only plain numeric ids), refuses on a
  read-only connection and for an own session without `own`, and records the statement in the
  query history (`Source::Session`).
- **Workbench** (`zen_workbench::database::sessions`): the `db.session.*` commands
  ([automation.md](automation.md#sessions-and-locks)); cancel and terminate always ask first with a
  `confirmExecute` refusal bound to the connection, action and session, and follow ADR-0172 for
  read-only connections and agents.
- **Window** (`ide/database_view/sessions.rs`, kind `sessions`, from **Sessions** in a connection's
  navigator menu): the table with PostgreSQL's or MySQL's columns, waiting (orange) and blocking
  (red) marks, the chosen session's query, the locks with the chains; Refresh, Show idle,
  Auto-refresh (off, 2, 5, 10, 30 s; only while the tab is drawn and the connection connected),
  Cancel Query / Terminate (Kill Query / Kill Session) with an inline question, the risk ticked
  before Terminate.
- **Not yet:** ClickHouse (`system.processes`, `KILL QUERY`) and Redis (`CLIENT LIST`, `CLIENT KILL`);
  MySQL metadata locks (`performance_schema.metadata_locks`); sorting and filtering the table;
  sessions of other databases on the same PostgreSQL server are listed but the IDE's own sessions
  are known only for this window's engine.

## Structure editing

- **The model** is `zen_db::alter`: a `Change` per action (create, rename, drop and comment schemas;
  create tables, views, sequences and routines; rename, drop and comment objects; add, alter and drop
  columns; add and drop primary, unique, check and foreign keys; create and drop indexes) and
  `alter::script(dialect, catalog, changes)`, which writes each change in the driver's dialect
  (ADR-0170) over a copy of the snapshot's catalogue that it updates as it goes, so a later change
  sees an earlier one (a column added, then indexed; a table renamed, then altered). The first change
  a server cannot make is refused with a `Reason` and its subject; nothing is guessed. SQL a person
  writes into a change (types, defaults, checks, a view's query, a routine's whole `CREATE`) must
  split into exactly one statement, and foreign key actions are checked against the five SQL ones.
- **PostgreSQL** gets one statement per attribute (`ALTER COLUMN … TYPE`, `SET`/`DROP NOT NULL`,
  `SET`/`DROP DEFAULT`, `COMMENT ON`, then `RENAME COLUMN`), `CASCADE` where asked, routines by
  their arguments. **MySQL and MariaDB** rename with `RENAME COLUMN`, change only a default with
  `ALTER COLUMN … SET DEFAULT`, and otherwise restate the column with `CHANGE COLUMN` from what was
  read (type, collation, nullability, default, `AUTO_INCREMENT`, comment), refusing (`restate`) when
  `SHOW CREATE TABLE` gives the column `ON UPDATE`, `INVISIBLE`, `SRID`, `COLUMN_FORMAT` or `STORAGE`;
  constraints are dropped as MySQL names them (`DROP PRIMARY KEY`, `DROP FOREIGN KEY`, `DROP INDEX`,
  MySQL's `DROP CHECK`); databases are the schemas; sequences only on MariaDB; no `CASCADE`.
  **SQLite** renames tables and columns and adds nullable or defaulted columns in place, and
  otherwise rebuilds the table as its documentation describes: `PRAGMA foreign_keys = OFF` before
  the transaction, `legacy_alter_table` on while the new table (`_zen_new_<name>`) is created from
  the model, the rows copied, the old table dropped and the new one renamed, then its indexes and
  triggers created again from the texts read. A rebuild is refused (`rebuildLoses`) when the table's
  definition has what the model does not keep (`CHECK`, `GENERATED`, `AUTOINCREMENT`, `COLLATE`,
  `STRICT`, `WITHOUT ROWID`, `DEFERRABLE`), and (`runFirst`) when a column of the table was renamed
  earlier in the same script and its indexes or triggers would be created from texts the rename
  changed. In one change the rebuild runs before the rename. ClickHouse and key-value drivers refuse
  (`driver`).
- **Staging** (`zen_workbench::database::alter`): changes are kept per object tab in the window
  (`Alterations`), checked when staged by generating the whole script again, and listed with a
  summary in words; a change whose table's details were not read yet asks for them (`loading`). The
  tab is dirty while anything is staged, and closing it is refused with `pendingStructureChanges`
  (the window asks to discard). Read-only connections refuse staging.
- **Running**: `db.structure.execute` builds `Script::all`, wrapping the statements in `BEGIN` …
  `COMMIT` when the dialect has transactional DDL and the Main context is in auto-commit outside a
  transaction (otherwise they wait for Commit, and the preview says so), passes them through
  `safety::guard` (read-only, agents, confirmations; `DROP` and `ALTER … DROP` always ask) and runs
  them in the connection's **Main** context, stopping and rolling back at the first error. The
  history source is `structure`, its place the object's name.
- **Reading again only what changed**: before running, `Structure::expect` stores the introspection
  request the next `StructureChanged` event of the connection uses instead of a refresh of every
  schema: the changed objects' details when every change is inside existing tables or objects
  (`IntrospectRequest::objects`), else the schemas touched (a created, renamed or dropped object, a
  schema created or renamed). After a run that ended well the changes are cleared, a tab whose object
  was dropped closes and one whose object was renamed opens again on the new path once it is read.
- **Migrations**: `db.structure.saveMigration` writes the preview, its notes as `--` comments, to a
  new file `<folder>/<yyyymmddhhmmss>_<slug>.sql` (UTC) of the project, `migrations` by default; the
  folder must be a relative path inside the project, and a file is created only if it does not exist.
- **Tests**: unit tests of every dialect's statements and refusals (`alter/tests.rs`), commands over
  the memory driver (`database/alter/tests.rs`), the real `sqlite3` rebuilding a table with rows, an
  index, a trigger and a view (`tests/structure.rs`), and ignored Docker tests running the scripts on
  PostgreSQL 15 and 18, MySQL 8.4 and MariaDB 11.4 and reading the result back.
- **Not yet:** editing a view's query, a routine's body, a sequence's settings or a type in place
  (they are created anew or changed in a console); triggers, policies, partitions and permissions;
  reordering columns; a column rename and a later rebuild of the same SQLite table in one script.

## Nothing lost after a crash

- **What was lost before.** A kill or a power loss lost a console's text typed since the last save
  (consoles are saved after a second of quiet, so continuous typing or moving the mouse put the save
  off), every staged data edit (they lived only in memory), and any trace of the transactions that
  were open; the history was appended without a sync, so a power loss could lose its tail or tear a
  line, and settings files were replaced without syncing.
- **The journal.** `zen_db::recovery::Recovery`, one per data folder and process
  (`Recovery::shared`), keeps `<data folder>/databases/recovery-<process>-<n>.jsonl` (owner-only,
  0600) and holds `recovery-<process>-<n>.lock` locked while it runs (ADR-0177), so an IDE and a
  headless one on the same data folder never read, offer, compact or delete each other's journal.
  When it starts it takes over the journals whose lock it can take (runs that stopped) and the
  `recovery.jsonl` of earlier builds: their items become its own and are offered back, and those
  files go once written to its own. Records are `{ key, item }` (no item:
  removed), each with a CRC-32 (`zen_db::journal`), appended at once on a thread of its own, synced
  at most once a second, compacted through a temporary file, a rename and a folder sync when dead
  records outweigh live ones, and deleted when nothing is kept. Items are a console's text
  (`console:<path>`), a result's staged edits (`edits:<window>:<result>`: the connection, the
  dialect, the `Editing` — table, key and columns — and the `Changes` with every changed row's
  values as read), and an open transaction (`transaction:<window>:<connection>:<context>`: counts
  and its last statements' first lines, passwords masked). No secrets: connection passwords never
  enter it, statements go through `history::redact`.
- **Capturing.** `zen_workbench::database::recovery::capture` runs after every logic step of each
  window (GUI and headless): it compares console versions, result revisions and transaction counts
  with what it last kept and only clones what changed; serialising and writing happen on the
  journal's thread. A console saved, edits saved or cancelled, a transaction ended: their records go.
  A clean exit (`recovery::finish`) forgets the open transactions, which the exit dealt with.
- **Recovering.** The journal left by the previous run is read in the background; everything in it
  is *recovered*. Once read, each window puts a recovered console's text into its open tab as an
  undoable edit (unsaved, the file untouched) and posts a notification; the rest is announced once
  with **Review** (`db.recovery.open`). The Recovered tab (`ide/database_view/recovery.rs`) and
  `db.recovery.list`, `.get`, `.restore`, `.discard` show edits as the SQL `edit::plan` writes in
  their dialect (the old key and the values read checked) and open it in a new console, never run;
  transactions are reported as rolled back by the server. Recovering connects nowhere. Proven by
  `crates/platform/db/tests/kill_and_restart.rs` (a child process killed with SIGKILL, then a torn
  record appended) and the workbench's restart test.
- **History storage off.** `db.history.settings { enabled: false }` (⚙ in the History tab) stops
  recording; clearing everything deletes its file.
## Database profiles

[ADR-0173](adr/0173-database-profiles-for-wire-compatible-servers.md). A profile is data over a
compiled-in driver (`zen_db::profile::PROFILES`); a connection keeps its id in `profile`, its
`driver` stays the base driver.

- **The catalogue.** Each profile has an id, name, base driver, icon (its family's glyph), default
  port, database and user, URI schemes of its own, managed-service host patterns, version-text
  markers, a TLS default, settings it turns on, quirks and a docs link. Quirks: `catalogue`
  (`native`, `informationSchema`, `metaFunctions`), `fingerprints`, `guards` (lock and
  idle-transaction timeouts and `default_transaction_read_only`), `transactions`, `allDatabases`
  and the navigator folders `hidden`. `Profile::capabilities` is the base driver's with the
  profile's schemes first, no transactions or isolation levels without `transactions`, and no
  catalogues without `allDatabases`.
- **Reading structure.** PostgreSQL sessions read the profile's quirks at open (the connection's
  profile, else a managed-service host) and again after `SELECT version()`; the
  `informationSchema` path (`postgres/catalogue.rs`) reads `information_schema.schemata`, `.tables`,
  `.columns` and the primary and unique keys (`table_constraints` with `key_column_usage`, best
  effort) over the simple protocol with names as literals, so no arrays, `xmin` or PostgreSQL-only
  functions; QuestDB's `tables()` and `table_columns()` (at most 500 tables' columns). MySQL
  sessions skip the `CRC32` fingerprints for StarRocks and Doris. The tree hides a profile's folders
  (`schema::tree`, by `Snapshot::profile`).
- **Detection.** `DriverSession::server_version` now names the server: PostgreSQL from `SELECT
  version()` (`PostgreSQL 18.0`, `CockroachDB CCL v24.1.0`, `PostgreSQL 8.0.2 (Amazon Redshift)`,
  `PostgreSQL 16.4 (timescaledb 2.17.0)` from `pg_extension`); MySQL from `VERSION()` and
  `@@version_comment` (`MySQL 8.0.11 (TiDB)`); Redis from `INFO server` (`valkey_version`,
  `dragonfly_version`, `garnet_version`, `keydb` in the text). `profile::detect` takes the host
  first, then the markers within the driver's family. The engine keeps the result as
  `ConnectionStatus::detected_profile` and `Snapshot::profile`, raises `Event::ProfileDetected` once
  (a notification with **Use … Profile**, `db.dataSource.update { id, profile }`), and Test
  Connection answers `test.profile`; the dialog shows **Use the … profile**, which changes only the
  dialog. Stored settings never change by themselves.
- **The dialog.** Drivers and profiles grouped by family (`ide-dbc-family`, "PostgreSQL and
  compatible"), a search over names, ids and families, a scroll list with the family's icon;
  choosing a profile fills its port, database, user, SSL mode and settings; a URL picks the profile
  by scheme or host; Edit shows the connection's profile.
- **Tests.** Unit tests of the table, URLs, hosts, detection, capabilities, version texts of every
  family, the `information_schema` queries and assembling their rows, the engine's detection event,
  `db.driver.list` and `db.dataSource.add` with profiles, and the dialog (offscreen PNGs). Only
  PostgreSQL, MySQL, MariaDB and Redis images are on the team's machines; the other servers are not
  run in Docker until their images are approved.

Facts were taken from the vendors' documentation:

| Profile | Port, database, user, TLS | Source |
|---|---|---|
| CockroachDB | 26257, `defaultdb`, `root`; `cockroachdb://` | [Connection parameters](https://www.cockroachlabs.com/docs/stable/connection-parameters) |
| YugabyteDB | YSQL 5433, `yugabyte`, `yugabyte` | [Default ports](https://docs.yugabyte.com/preview/reference/configuration/default-ports/) |
| TimescaleDB | 5432; `*.tsdb.cloud.timescale.com` | [psql](https://docs.timescale.com/use-timescale/latest/integrations/psql/) |
| Neon | 5432, `neondb`; TLS required; `*.neon.tech` | [Connect securely](https://neon.tech/docs/connect/connect-securely) |
| Supabase | 5432, `postgres`, `postgres`; TLS; `*.supabase.co`, `*.pooler.supabase.com` | [Connecting to Postgres](https://supabase.com/docs/guides/database/connecting-to-postgres) |
| Amazon Redshift | 5439, `dev`, `awsuser`; TLS | [Configuring connections](https://docs.aws.amazon.com/redshift/latest/mgmt/configuring-connections.html) |
| Greenplum | 5432, `postgres`, `gpadmin` | [Greenplum 7](https://techdocs.broadcom.com/us/en/vmware-tanzu/data-solutions/tanzu-greenplum/7/greenplum-database/landing-index.html) |
| AlloyDB | 5432, `postgres`, `postgres`; TLS | [Connect with psql](https://cloud.google.com/alloydb/docs/connect-psql) |
| Citus | 5432; the `citus` extension | [Citus docs](https://docs.citusdata.com/en/stable/) |
| Materialize | 6875, `materialize`, `materialize` | [SQL clients](https://materialize.com/docs/integrations/sql-clients/) |
| RisingWave | 4566, `dev`, `root` | [Quick start](https://docs.risingwave.com/get-started/quickstart) |
| QuestDB | 8812, `qdb`, `admin`; meta functions | [PostgreSQL wire](https://questdb.com/docs/reference/api/postgres/), [Meta functions](https://questdb.com/docs/reference/function/meta) |
| CrateDB | 5432, `doc`, `crate` | [PostgreSQL wire protocol](https://cratedb.com/docs/crate/reference/en/latest/interfaces/postgres.html) |
| TiDB | 4000, `root`; `*.tidbcloud.com` | [Connect to TiDB](https://docs.pingcap.com/tidb/stable/dev-guide-connect-to-tidb) |
| SingleStore | 3306, `root`; `*.svc.singlestore.com` | [Connect to SingleStore](https://docs.singlestore.com/cloud/connect-to-singlestore/) |
| PlanetScale | 3306; TLS verified; `*.psdb.cloud` | [Connection strings](https://planetscale.com/docs/vitess/connecting/connection-strings) |
| Vitess | vtgate 15306 in the local example | [Local install](https://vitess.io/docs/get-started/local/) |
| StarRocks | FE 9030, `root` | [Quick start](https://docs.starrocks.io/docs/quick_start/shared-nothing/) |
| Apache Doris | FE 9030, `root` | [Quick start](https://doris.apache.org/docs/gettingStarted/quick-start) |
| Percona Server | 3306, `root` | [Quickstart](https://docs.percona.com/percona-server/8.4/quickstart-overview.html) |
| Aurora MySQL | 3306, `admin`; cluster endpoints `*.cluster-*.rds.amazonaws.com` | [Connecting to Aurora](https://docs.aws.amazon.com/AmazonRDS/latest/AuroraUserGuide/Aurora.Connecting.html) |
| Valkey | 6379; `valkey://`, `valkeys://` | [valkey-cli](https://valkey.io/topics/cli/) |
| KeyDB | 6379 | [KeyDB docs](https://docs.keydb.dev/docs/) |
| Dragonfly | 6379 | [Getting started](https://www.dragonflydb.io/docs/getting-started) |
| Garnet | 6379 (`defaults.conf`) | [Getting started](https://microsoft.github.io/garnet/docs/getting-started) |
## External drivers

A database the IDE does not compile in comes as a **driver program** of a plug-in
([ADR-0174](adr/0174-external-database-driver-protocol.md), the protocol in
[database-drivers.md](database-drivers.md)).

- **`zen_db::external`**: `ExternalDriver` implements the `Driver` trait over a program that speaks
  JSON-RPC 2.0, one message per line on its standard streams; sessions are the driver's sessions
  inside it. Rows are pulled with `query` and `fetch` as the result store wants them (with
  `serverPaging`), or read with `execute` up to the row cap. The engine finds external drivers
  through `Engine::set_more_drivers` after its own; a compiled-in id is never replaced.
- **Supervised**: a program starts with the first session of a connection (`perConnection`) or of
  the driver (`shared`) after its SHA-256 is checked, and gets `shutdown` with its last session, killed
  if it does not exit within two seconds. `initialize` has 10 s, control requests 30 s. A program
  that dies fails what waits with a lost connection and the engine's reconnect starts a new one;
  three such stops within a minute keep it from starting for that minute. Everything runs on the
  engine's runtime.
- **Secrets**: the password travels in `connect` only, from a buffer wiped after it is written; the
  driver's log (`db.driver.log`) holds lifecycle lines, standard error and a line for each failed
  `connect` (`connecting <id> failed: <reason>`) with the password and URI credentials removed,
  and never request or answer bodies. A query on a connection whose last connect failed reports
  that failure (`Engine::cell`), not "not connected". A driver whose capabilities ask no password
  (`password: false`, DuckDB) gives new connections `auth: none`, in `db.dataSource.add` and the
  connection dialog.
- **Plug-ins**: `"databaseDrivers"` in `plugin.json` (`zen_plugins::CatalogPlugin::database_drivers`,
  checked as `zen_db::external::DriverDeclaration`); installing or updating a plug-in on the user's
  word downloads its program for this computer to `<plug-ins>/<plugin>/drivers/<driver>/` and checks
  it (`Installed::install_driver_program`); the update at start skips such plug-ins. The IDE's
  `Plugins` keep the declared drivers (`zen_db::external::Declared`) and every window's `Database`
  takes them, so a plug-in installed or switched off changes the list at once.
- **In the workspace**: `db.driver.list` lists them after the compiled-in drivers with `external`
  (plug-in, installed, icon, process model, protocol, running programs); the connection dialog shows
  them from that list, with the fields their capabilities declare. Production safety, history,
  export, editing and the navigator work through the declared capabilities: SQL languages are judged
  by our tokens; other languages name their reading verbs (`readVerbs`).
- **Reference driver and conformance**: `zen-db-reference-driver` (a `[[bin]]` of `zen-db`, in no
  product) is an in-memory SQL database with test statements (`SLEEP`, `CRASH`, `STDERR`, `FAIL`);
  `cargo test -p zen-db --test external_driver` checks the protocol against it, or against any
  program named by `ZEN_DB_DRIVER`.
- **Not yet**: publishing driver programs on our release server with `cargo xtask release plugin`;
  archives with several files per platform; a dedicated icon per driver in the dialog (the
  manifest's `icon` is listed but the dialog shows its generic icon).

## First-party driver programs

The team's own driver programs are crates in `drivers/` ([ADR-0175](adr/0175-first-party-database-driver-programs.md)),
checked by the gate like the IDE: no C, TLS through rustls with RustCrypto in the program
(`zen-db-driver-kit`, `drivers/kit`), each with a plug-in in `plugins/<id>` that declares it.

- **SQL Server and Azure SQL** (`drivers/sqlserver`, driver `sqlserver`, plug-in `plugins/sqlserver`):
  `tiberius` 0.13 without its TLS features. TLS modes: Disable (refused when the server forces
  encryption), Prefer, Require, Verify CA and Verify Full; TDS 7.x encryption is negotiated in the
  pre-login exchange, whose TLS handshake (TLS 1.2) the program runs inside PRELOGIN packets, after
  setting the encryption option tiberius sends; the `strict`
  setting (or `tls.direct`, which the dialog offers for PostgreSQL only) is TDS 8.0, TLS first with ALPN `tds/8.0`. Logins with a password or a Microsoft
  Entra ID access token (`authentication: accessToken`, the token as the password); Azure SQL's
  redirection is followed. Databases, schemas, tables, views, procedures, functions and sequences
  are read from `sys.*` into the snapshot (columns with declared types, identity and computed
  columns, keys, foreign keys with their actions, checks, indexes, definitions, `MS_Description`
  comments, row counts), one fingerprint per schema (objects and their latest `modify_date`). Results
  are read page by page from the open TDS stream; closing early or Cancel sends TDS's attention
  signal; `timeoutMs` cancels the same way. Manual commit is `IMPLICIT_TRANSACTIONS ON`, the state
  comes from `@@TRANCOUNT`; isolation levels READ UNCOMMITTED to SERIALIZABLE and SNAPSHOT. Results
  of a `SELECT` name their table columns (`sys.dm_exec_describe_first_result_set`), so the grid
  edits rows by primary key, saved with `@P1` parameters in the T-SQL dialect (`SqlDialect::SqlServer`):
  brackets, `N'…'`, `0x…`, `1`/`0`, `CASE` for `NULL` order, `CONVERT(VARBINARY(MAX), @P1, 2)` for
  bytes. Sessions set what SQL Server's own tools set (`ANSI_NULLS`, `QUOTED_IDENTIFIER`,
  `ARITHABORT`, `TEXTSIZE`…). Error positions are the start of the line the server names.
- **Not yet for SQL Server**: publishing its program (so the plug-in has no platforms and no
  release yet); Windows logins (NTLM, Kerberos); named instances through SQL Browser (give the port);
  read-only sessions on the server (SQL Server has none; the engine judges statements);
  `PRINT` and `RAISERROR` messages as notices (tiberius does not expose them); results after the
  first of a batch; explain plans; DDL preview; TDS 8.0 against a real server (SQL Server 2022 on
  Linux refused our test certificate for strict connections; the program's side is unit-tested).
- **SQL Server, tested live** (2022, 16.0.4305, in Docker): its ignored tests and the conformance
  suite pass, also against a server that forces encryption with a CA-signed certificate and offers
  TLS 1.3. They found four faults, fixed: tiberius built without TLS refused every TLS mode
  (`compiled without a TLS backend`), so the program now sets the pre-login encryption option
  itself; the server's own certificate is X.509 version 1, which webpki refuses even when nothing is
  checked (`require`), so the handshake signature is verified with the key read from it; TLS 1.3
  inside the pre-login broke the stream, so it is TLS 1.2 there; and after Cancel or an early close
  tiberius did not find the attention's acknowledgement in the server's second message and lost the
  session, so the program joins the two. An error in a later statement of a batch was also lost
  with its position; it is now read.
- **MongoDB** (`drivers/mongodb`, driver `mongodb`, plug-in `plugins/mongodb`; docs/database-drivers.md
  has the details): a client of our own over `bson` (ADR-0175, decision 5): OP_MSG, `hello`,
  SCRAM-SHA-256 and SCRAM-SHA-1, the primary of a replica set, `mongodb://` and `mongodb+srv://`
  (SRV and TXT looked up by the program), TLS from the first byte in the usual modes. The navigator
  shows databases, collections, views and indexes; consoles take the MongoDB shell's statements
  (`db.orders.find({ total: { $gt: 10 } }).sort({ total: -1 })`, `insertOne`, `updateMany`,
  `aggregate`, `createIndex`, `show dbs`, `use`, `db.runCommand(…)` …), split as a command language
  that spans lines (`Dialect::Script`: `;` and blank lines, JavaScript quotes and comments); results
  are documents as rows (top-level fields as columns, nested documents and arrays as JSON) read page
  by page from server cursors; Cancel kills the operation with `killOp`; a read-only connection's
  session refuses changes in the program. Production safety and agents limited to reading judge
  statements by the method they call (`find`, `countDocuments`, `distinct`, `show` … read; the
  engine takes the method of a call chain as the verb, `zen_db::external::command_verb`). Tested
  live against `mongo:8.0`, with TLS against a server that requires it.
- **MongoDB documents in the grid**: a result whose statement is a `find` or `findOne` of one
  collection (`zen_db::edit::document::collection_of`: `db.c.find(…)`, `db.getCollection("c")`,
  `db.getSiblingDB("d").getCollection("c")`, with cursor modifiers only) is editable, its rows
  identified by `_id` (`Capabilities::row_identity` `_id` with the language `mongo`); `_id` itself
  is read-only. Typed input follows the field's BSON type (`edit::document::parse_input`); Save runs
  shell statements instead of SQL (`edit::document::plan`): `deleteOne` by `_id`, `insertOne` with
  an ObjectId the IDE makes (read back with a `find` projected on `_id`), `updateOne` by `_id` and
  the old values of the changed scalar fields with `$set`, and `$unset` for a field set to its
  default. A driver without transactions saves without `BEGIN` or savepoints, so a failure leaves
  the statements before it saved (`SaveReport::kept`, said in the message). An update counts the
  documents it matched, not those it modified, so a value set to what it was is not a conflict.
  The shell reads Extended JSON wrappers (`{"$oid": …}`, `{"$date": …}`) back as the values they
  stand for, so nested documents keep their types. The results' **JSON** view
  (`db.result.view` `json`, offered when `db.result.editState` says `documents`) shows each row as a
  document in the shell's syntax, absent fields left out (`edit::document::shell_document`). The
  navigator's collections and views open a console with `db.getSiblingDB("d").getCollection("c").find({})`
  (`Driver::open_text` of an external driver of the `mongo` language). There is no tree of a
  document's fields; the value panel's folding JSON serves for one field.
- **MongoDB transactions and X.509**: manual commit runs statements in a transaction of a logical
  session the program keeps (`commit`/`rollback` end it; replica sets and sharded clusters only),
  and `authMechanism` `MONGODB-X509` logs in with the TLS client certificate. Both tested live
  (a one-member replica set, and a server requiring TLS with a CA-signed client certificate).
- **Not yet for MongoDB**: compression, AWS, LDAP and Kerberos logins, reading
  from secondaries, fields that appear only after the first batch as columns, and `mongodb+srv://`
  on Windows.
- **DuckDB** (`drivers/duckdb`, driver `duckdb`, plug-in `plugins/duckdb`; docs/database-drivers.md
  has the details): the program drives the `duckdb` command-line program the user installs, one
  process a session (ADR-0175, decision 6): database files and in-memory databases, Parquet, CSV and
  JSON read by DuckDB's own SQL, queries described first so columns are typed and an empty result
  has them, transactions with manual commit, Cancel and timeouts (SIGINT ends the process; the next
  statement starts it again), and a tree of its own (databases, schemas, tables, views, columns),
  whose tables open as a console with their `SELECT` (`Driver::open_text` of an external SQL driver
  without introspection). Statements are checked by the program, so a dot-command or a second
  statement never reaches `duckdb`. Tested against `duckdb/duckdb` 2.0 in Docker.
- **Not yet for DuckDB**: editing results, results read page by page, and Cancel that keeps an
  in-memory database.
- **Oracle** (`drivers/oracle`, driver `oracle`, plug-in `plugins/oracle`;
  [ADR-0190](adr/0190-oracle-through-sqlcl-the-user-installs.md), docs/database-drivers.md has the
  details): no pure-Rust client exists, so the program drives **SQLcl**, which the user installs
  with a JDK, one process a session, started silent, restricted and not logged in; the password
  goes on its standard input in `CONNECT`, never in arguments. SQLcl is found from the connection's
  **program**, `SQLCL_HOME`, `ORACLE_HOME/sqlcl`, `PATH` and Homebrew; Java from **javaHome** or
  SQLcl's own lookup; a missing one is an error with code `sqlclMissing` or `javaMissing`.
  Connections name a service, a SID, a TNS alias (`TNS_ADMIN` from **tnsAdmin**) or a connect
  string; TLS goes over TCPS (host name checked only for Verify Full, a wallet folder or an
  Autonomous Database wallet `.zip`); **role** connects as SYSDBA and the like. Results come from
  `SET SQLFORMAT json` (typed columns, exact numbers, `SET ROWLIMIT` for the row cap), changes are
  counted from the feedback, errors keep their Oracle code and position. Only SQL and PL/SQL reach
  SQLcl: its own commands and lines it would act on are refused. Manual commit follows SQLcl's
  `AUTOCOMMIT`; saving edits turns it off for its own transaction. Cancel sends SIGINT and ends
  SQLcl when it does not stop within three seconds (the next request connects again). The
  navigator and completion read schemas, tables, views, materialised views, packages (a new
  object kind with the folder **Packages**), procedures, functions, sequences and types with
  columns, keys, indexes, comments and sources from the `ALL_*` views, and a query of one table
  gets its columns' table, so its rows are edited. Generated SQL has an Oracle dialect
  (`SqlDialect::Oracle`) and scripts are split by `Dialect::Oracle` (`q'[…]'`, PL/SQL ending at a
  `/` line). Tested with a fake SQLcl and the conformance suite; ignored tests run a real SQLcl
  against `gvenzl/oracle-free`.
- **Not yet for Oracle**: results read page by page, bind parameters (values are inlined),
  `DBMS_OUTPUT`, execution plans, a CA file or client certificate from the TLS settings (use a
  wallet or Java's trust store; Require checks the certificate as Verify CA does), Prefer falling
  back to TLS, Kerberos, OS and proxy logins, triggers, synonyms and database links in the
  navigator, inserting a row of defaults only, data compare of tables with a key of several
  columns, and Cancel on Windows (it ends SQLcl).
- **Publishing driver programs**: `cargo xtask release plugin` builds only the WebAssembly module, so
  the driver plug-ins cannot be installed from Plugins yet.

## Command-line clients and code types

- **The model** (`zen_db::client`, desktop only): `tools_for(driver)` names the clients in order
  (PostgreSQL `psql`; MySQL `mysql`, then `mariadb`; MariaDB the other way round; SQLite `sqlite3`;
  Redis `redis-cli` or `valkey-cli`; ClickHouse `clickhouse-client` or `clickhouse client`), and
  `build` writes the arguments from the connection's settings: `--host=`/`--port=`/`--username=`/
  `--dbname=` for psql with TLS in `PGSSLMODE`, `PGSSLROOTCERT`, `PGSSLCERT`, `PGSSLKEY` and
  `PGSSLNEGOTIATION`, and a read-only connection's `PGOPTIONS=-c default_transaction_read_only=on`;
  `--protocol=TCP` and `--ssl-mode=` for mysql, `--ssl` / `--skip-ssl` /
  `--ssl-verify-server-cert` for mariadb; `-readonly` for sqlite3; the first node, `-c` for a
  cluster, `-n`, `--tls` and `--insecure` for redis-cli; `--secure` for ClickHouse, whose port is
  left to the client (the driver's is HTTP). A connection through SSH uses the loopback address of
  its open tunnel (`Engine::tunnel_open`, never opening one); ClickHouse through SSH and Redis
  Sentinel are refused. A connection read-only by itself or by its type gets a read-only client
  session where the client can have one (`PGOPTIONS`, mysql's `--init-command=SET SESSION
  TRANSACTION READ ONLY`, sqlite3's `-readonly`, ClickHouse's `--readonly=1`; redis-cli has none).
  The password is never an argument or a variable of the client: `Tool::password_way` names a file
  the client reads (`PGPASSFILE` with a `*:*:*:*:` line, `--defaults-extra-file` with a `[client]`
  section, `--config-file` with ClickHouse's `<config>`; `Tool::password_file` writes each, escaped)
  or `--askpass` for redis-cli.
- **Open in Terminal** (`zen_workbench::database::clients`, `db.connection.openTerminal`, in a
  connection's navigator menu): the client is looked for on `PATH` and then in the folders installers
  use (an app started from the Dock has a short `PATH`), and runs through the platform terminal
  (`Terminals::run_in_new_tab`, a tab named `psql: Shop` that is not saved with the window) as
  `/bin/sh -c '"$@"; rm -f -- "$secret"; unset …; exec "$shell" -l'`: the client first, then the
  user's login shell, so the client's last words stay readable and nothing reaches the scrollback.
  The password file is created new (`O_EXCL`, mode 600) in the user's temporary folder and removed
  when the client ends, or by a trap when the tab closes; on Windows no file is written and the
  client asks. Relative entries of `PATH` are skipped, so a program in the project's folder is never
  taken for the client. The password is the one given, else the connected connection's (`Engine::password`),
  else the saved one read on zen-db's runtime (operation `readSecret`; `clients::step` starts the
  client when it ends, never on the window's thread), else none and the client asks. Agents get a
  client only where their access is full and the connection is neither read-only nor asks before
  changes (none of the consoles' checks hold in a client), and never with a stored password, because
  they can read the terminal.
- **Copy Client Command** (`db.connection.clientCommand`): the same command as one line for a POSIX
  shell, with the TLS variables and without the password.
- **Generate Code ▸ Go Struct, Rust Struct, TypeScript Interface** (`zen_db::codegen`,
  `db.object.generateCode`, in a table's or view's navigator menu) copies a type made offline from
  the snapshot's columns: Go with `db` and `json` tags and Go's initialisms (`UserID`), pointers for
  nullable columns, `time.Time`, `[]byte`, `json.RawMessage`; Rust with snake-case fields, raw
  identifiers for keywords and `Option`; TypeScript with the column names as they are and
  `| null`. Types follow each driver: PostgreSQL arrays, MySQL `unsigned` and `tinyint(1)`,
  ClickHouse `Nullable`, `LowCardinality` and `Array`, SQLite affinity; decimals, 64-bit integers in
  TypeScript and anything unknown are strings. Column and table comments become doc comments.
- **Not yet:** crate- and package-specific types (chrono, uuid, rust_decimal, serde derives; Go's
  `sql.Null*`); more languages (Kotlin, Python dataclasses); choosing the type name; clients in an
  external terminal; MySQL and MariaDB read-only sessions in the client; a pane next to the open
  terminal tab rather than a new tab.

## Migrations

Migrations follow the tools' own documentation and how they record what is applied.

- **Recognising** (`zen_db::migrations`, files only): `recognise(root, folder)` names the sets a
  folder holds — Flyway `V<version>__<name>.sql` (and `flyway.conf`/`flyway.toml`), Liquibase
  changelogs, golang-migrate `.up.sql`/`.down.sql` pairs, goose `-- +goose Up`, Atlas beside
  `atlas.sum`, sqlx in a Rust project depending on sqlx, Diesel `<version>_<name>/up.sql`, Alembic
  `versions/*.py` with `alembic.ini`, Django `<app>/migrations/<number>_<name>.py` beside
  `manage.py`, Prisma `<time>_<name>/migration.sql`, Rails `db/migrate/<time>_<name>.rb`, Knex with
  a `knexfile`. Files over 256 KB are not read; at most 5000 migrations a folder. Versions compare
  as numbers (`natural`).
- **Scanning** (`zen_workbench::database::migrations`): a thread walks the project seven folders
  deep and 30 000 entries at most, skipping hidden, dependency, build, excluded and Git-ignored
  folders, and is cancelled when the project changes. A changed file whose path looks like a
  migration or a tool's configuration marks the sets stale; the next look scans again.
- **State**: `history_query` reads the tool's table (`flyway_schema_history`,
  `databasechangelog`, `schema_migrations`, `goose_db_version`, `atlas_schema_revisions`,
  `_sqlx_migrations`, `__diesel_schema_migrations`, `alembic_version`, `django_migrations`,
  `_prisma_migrations`, Knex's `knex_migrations`) in the Metadata context, only on a connection that
  is already connected, at most 100 000 rows in 30 seconds; a missing table means nothing is
  applied. `statuses` joins it with the files: applied, pending, failed (dirty or not finished) or
  unknown, and those the database has without a file.
- **Running** (`zen_db::migrations::command::invocation`, `MigrationRun`, run kind
  `db.migration`): the tool's command line for up and down against the connection's host, port,
  database, user and TLS mode, in the folder the tool runs in; the program only from `PATH` folders
  outside the project and the folders installers use, never one the project brings (ADR-0193:
  `.venv/bin`, `bin/rails`, `node_modules/.bin`, `npx`). The password (read from the Keychain on a
  thread before the run starts, kept in memory for this IDE's later runs) is never in the tool's
  environment or arguments: it goes in a new 0600 file the tool is pointed at — `.pgpass` in
  `PGPASSFILE` for golang-migrate, goose, Atlas, sqlx and Diesel on PostgreSQL, a `-configFiles=`
  for Flyway, a `--defaults-file=` for Liquibase — removed by the `/bin/sh` wrapper that runs the
  tool (which also unsets inherited `PGPASSWORD`, `MYSQL_PWD`…) however it ends. Tools that take a
  password only in a URL (Alembic, Django, Prisma, Rails, Knex, and the MySQL forms) are refused
  on connections with a password. A run starts only with a one-time approval `db.migration.run`
  gives after its checks; a saved configuration, Rerun or `run.process.start` is sent back to it.
  Connections through SSH (whose tunnel only the IDE uses) and other drivers are refused.
- **Safety** (ADR-0172): read-only connections refuse; a closed connection refuses
  (`notConnected`, with a hand-off to connect: the pending migrations come from the applied state);
  down always asks, up where the type confirms execution; agents follow the type's agent access. `db.migration.status` answers
  `readOnly`, and the tab turns Up and Down off with the reason in their tooltip.
- **Table layout**: the State, Version and Name columns keep their widths until the tab is narrower
  than all four columns need, then every column shrinks in proportion; cut cells end with "…", and a
  file cell shows its whole path on hover.
- **After a run**: `connections::step` calls `migrations::step`, which notices the run ended, reads
  the connection's structure again (`structure::refresh`) if it is connected, and reads the
  applied state again.
- **The tab** (`ui::ide::database_view::migrations`): the set and connection pickers, Refresh, Up
  and Down, a summary line and the migrations with their state in theme colours; opened from a
  connection's navigator menu (**Migrations**) or `db.migration.open`.

## Features

"Have" means Zen IDE has it today; "Stage" is the stage of the plan that brings it (see
[Delivery order](#delivery-order)). Stage A is the workspace host and this empty workspace, B the
foundations (the engine, connections and secrets), C the daily work, D the second tier, E later.

### The workspace and consoles

| Feature | Have | Stage |
|---|---|---|
| The workspace: navigator on the left, editors in tabs, results under the editor | The Database workspace over the editor area, with a toolbar, a left panel slot and inner tabs; collapses to the code and back with every tab in place, also after a restart | A (built) |
| First actions when nothing is open | The Overview tab, always first and not closable: "No connections yet" with New Console | A (built) |
| SQL consoles (not project files) and SQL scripts (files of the project) | Consoles are files in the IDE's data folder, `consoles/<project key>/<connection>/console.sql`, `console_1.sql`…, real editors (typing, undo, find, highlighting from the SQL plug-in, offered when missing); an empty console is deleted on close; consoles are in Recent Files and the Switcher | A (built), with the connection folder `local` until connections exist |
| Recent scripts | Recent Files (⌘E) lists consoles | A (built) |
| Scripts bound to a connection, kept per project | Any `.sql` file of the project bound to a connection and schema (kept per project in the data folder), run from the code editor's bar, keys and gutter into the workspace; scripts per connection and New Script in the navigator; a script's tab moves between the workspace and the editor by dragging | C (built, E8OcZMxb); moving tabs, A (built, sXs4vGek) |
| Editors side by side | The workspace's area splits right and down as the main editor's, each part with its own tabs, kept across restarts | A (built, sXs4vGek) |
| Local history of scripts | Local History keeps consoles' versions as project files' | A (built, sXs4vGek) |
| A console per connection, opened from the navigator | Open Console on a connection or object in the navigator | C (built) |

### Connections and connection types

| Feature | Have | Stage |
|---|---|---|
| Creating a connection; Test Connection | One dialog: drivers on the left, the driver's fields beside (Main, SSH/SSL, Advanced, General), Test Connection with the server's version and round trip | C (built) |
| Connection import and detection of `.env` and Compose files | Import from DBeaver's `data-sources.json` and JetBrains `.idea/dataSources.xml` without secrets; `.env` and Compose detections offered on the Overview and in the dialog, never connected (ADR-0165, decisions 5 and 6) | C (built) |
| Drivers | Drivers compiled in, pure Rust, no downloads (ADR-0164): PostgreSQL 12–18, MySQL 8+ and MariaDB 10.6+, Redis and Valkey, ClickHouse built; 25 wire-compatible databases as profiles over them (ADR-0173); SQL Server, MongoDB, DuckDB and Oracle (through SQLcl, ADR-0190) as external driver programs of plug-ins (ADR-0175) | B (PostgreSQL, Redis, ClickHouse built), C (MySQL and MariaDB built) |
| Connections in folders | Global and project connections, in folders, with personal overrides (ADR-0165) | B (built) |
| Connection types (Development, Test, Production) with safety defaults and colours | Colours from the theme (a dot on the Overview), tabs coloured by type, a Production frame and type badge on consoles, read-only types, what agents may run per type (ADR-0172) | B (built), C (tabs, safety built) |
| Read-only connections | Read-only on the server and refused by the engine for every driver, the toolbar's lock | C (built) |
| Passwords | The macOS Keychain (ADR-0165): `zen-secrets`, read once per session on `zen-db`'s runtime when connecting | B (built) |
| SSH tunnels | The system's OpenSSH with `~/.ssh/config`, its agent, ProxyJump and multiplexing; questions through our askpass dialog, secrets in the Keychain | C (built) |
| Connect, Invalidate/Reconnect, Disconnect, Disconnect All | As commands and on the Overview; nothing connects by itself | B (built) |
| Separate connections for metadata and each editor; keep-alive; reconnect on failure | Main, Metadata and one session per console; ping before reuse; reconnect with backoff restoring tracked state, reads retried once, writes never | B (built) |

### The Database Navigator

| Feature | Have | Stage |
|---|---|---|
| A tree: connection → databases → schemas → folders per object kind → objects → columns, keys, indexes | In the left panel, from a snapshot kept on disk at once, read in the background with fingerprints, never on the UI thread | C (built) |
| Filters per connection and per folder; show connected only | Include and exclude masks per kind from the connection, a filter box over what was read, Show Connected Only | C (built), D (per folder) |
| Link with Editor | Selects the connection of the console in front | C (built) |
| Context menu | Open Console, View Data, Generate SQL (DDL, SELECT, INSERT), Copy Name, Copy Qualified Name, Edit Connection, Connect, Reconnect, Disconnect, Refresh, each a command | C (built), D (Create, Rename, Delete, Export, Tools) |
| Search for a database object by name | Go to Database Object (⌘O in the workspace) and a Database tab in Search Everywhere, from every snapshot, offline, with scripts and the query history | D (built) |
| Every database of a PostgreSQL server | All databases listed; the connection's database is read, the others through their own connection | C (listed), D (read) |
| Generate DDL of tables, views, procedures | DDL from the snapshot, offline, with toggles for nested objects, permissions, separate keys, comments and partitions (`db.object.ddl`) | C (built) |

### The SQL editor and execution

| Feature | Have | Stage |
|---|---|---|
| Run the statement at the caret or the selection | ⌘⏎, no chooser popup | C (built) |
| Run a script; each statement in a new tab | ⌥⌘⏎ and ⌃⌘⏎ | C (built) |
| Completion of keywords, tables and columns from the connection | From the snapshot, offline, with aliases, CTEs and subqueries resolved and joins from foreign keys; ⌘B/F4 to the object, F1 and ⌘-hover describe it, unknown names are warnings | C (built) |
| Templates | The IDE's live templates for SQL | D |
| `:name` and `?` parameters | Real bind parameters: the Bind Parameters dialog, typed values remembered per console; `${var}`, `@set`, `@include` and a Variables tab | C (built) |
| Explain plan | ⌥⌘E: a Plan tab with the tree, hot nodes, details and the text; ANALYZE rolled back and checked like a run | D (built) |
| Format SQL | Reformat Code (⌥⌘L), options in Code Style → SQL | C (built) |
| Confirm SQL execution; a warning for DELETE and UPDATE without WHERE | Asked before anything runs, one statement at a time; DELETE/UPDATE without WHERE, DROP and TRUNCATE ask on every type and need their risk ticked (ADR-0172) | C (built) |
| Run a script on several connections | Execute on Several Connections: in turn, stop at the first error or go on, one confirmation for every connection, results grouped per connection | D (built) |
| Cancel the running statement; a statement timeout | Every operation cancellable, with a deadline (ADR-0164); cancel reaches the server within a second; Production sets a statement timeout | B, C (built) |
| Keyword case | Never changed while typing; completion puts keywords in the case typed; Reformat Code applies the keyword case of Code Style → SQL | C (built) |
| SQL in code | SQL inside the string literals of code: highlighted, completed and checked on the connection's snapshot (ADR-0183) | E (built) |
| Running SQL from code | Running the SQL of a string literal from the code editor (⌘⏎, gutter, ⌥⏎) on the file's connection, with bind parameters and the type's safety | C (built) |

### Results and the data editor

| Feature | Have | Stage |
|---|---|---|
| Results in tabs under the editor; statistics after scripts | Result tabs; statistics as a header line, no tab takes the focus | C (built) |
| The grid, record view, text view | The grid, the record view and the text view | C (built) |
| The WHERE filter box with history and completion | ⌥⌘F, saved filters standard | C (built) |
| Staged edits written on Save in one transaction; no unique key, no editing | With an SQL preview when the connection type asks, real bind parameters and conflicts reported | C (built) |
| The next page | Streams on from the open portal inside your transaction, read again past a cap outside it, view state kept (ADR-0168) | C (built) |
| Dates and times | Shown as the server sends them, `timestamptz` in the session's zone | C (built) |
| Advanced Copy and Advanced Paste | ⇧⌘C, ⌥⇧⌘V, plus Copy As code literals and SQL `IN` lists, background copies and a paste preview | D (built) |
| Value panel, JSON view, references panel | Value panel with Text, JSON (folding), XML, Hex and Image, and Calc, standard; references panel later | D (value panel and Calc built) |
| Navigate to a referenced row, references of a row, back and forward, dictionary lookup when editing | Links in key cells, References ▸, Back and Forward between data tabs, a lookup of up to 200 rows with search | D (built) |

### Transactions

| Feature | Have | Stage |
|---|---|---|
| Auto-commit, manual and smart commit; Commit and Rollback with a pending-statement counter | In the toolbar (⌃⌘K, ⌃⌘Z); smart commit off by default; isolation per console | C (built) |
| Idle transactions ended and connections closed after the type's times | With a 10-second countdown before the rollback and a notification; connections also close when the computer sleeps and reopen on use | B (built) |
| The transaction log | From the toolbar's counter; Pending Transactions across connections | C (built) |
| Asking before uncommitted changes are lost | For closing a console, disconnecting, reconnecting, switching its connection and leaving manual commit | C (built) |

### Object editors, history and sessions

| Feature | Have | Stage |
|---|---|---|
| An object editor | Data · Properties · DDL · Diagram, opening on Data | C, D, E (built) |
| An SQL preview before structure changes | With Execute, Copy, Open in Console and Save as Migration; changes accumulate | D (built) |
| ER diagrams | Drawn, read-only (built) | E |
| Search of data and names on the server | Find in Database: a schema's rows and names searched on the server, one table at a time with limits and deadlines, results opened as filtered data | D (built) |
| Query history | Every statement (consoles, filters, data tabs, saves, transactions, agents), searchable, standard | D (built) |
| Sessions | A Sessions tab per connection: PostgreSQL, MySQL and MariaDB sessions, blocking chains and locks, Cancel Query and Terminate after a question | D (built) |
| Migrations | Migrations tab: the project's migration folders recognised (Flyway, Liquibase, golang-migrate, goose, Atlas, sqlx, Diesel, Alembic, Django, Prisma, Rails, Knex), their state read from the tool's table, Up and Down run in the Run window, the structure read again after | E (built) |
| Client tools and generated code | Open in Terminal: the connection's own client (psql, mysql, mariadb, sqlite3, redis-cli, clickhouse) in the IDE's Terminal, the password only in a file of the user's own; Copy Client Command; Generate Code: Go, Rust and TypeScript types from a table | D (built) |

### Data transfer and comparison

| Feature | Have | Stage |
|---|---|---|
| Export and import | Export in one dialog, streamed (CSV, TSV, JSON, SQL, Markdown, HTML, XML, Parquet); CSV import in a five-step dialog, streamed, COPY on PostgreSQL | E (export and CSV import built) |
| Backup and restore with the database's client tools | pg_dump, pg_restore, psql, mysqldump and mysql found on PATH, in Homebrew or in the database's Docker container, run in the Run tool window with options per tool (Z7bt46TB) | D (built) |
| Data transfer between databases: table mapping, create target tables, batches, background | Transfer Data in four steps, any connection into any other, types mapped between dialects, streamed in batches, COPY on PostgreSQL | E (built) |
| Schema and data comparison | Compare tab: schemas of connections or saved snapshots, DDL side by side, sync script; rows by key | E (built) |

### Non-relational databases

Each follows the database's own documentation.

| Feature | Have | Stage |
|---|---|---|
| Redis and Valkey: keys by separator, value editors per type, a command console | The navigator, tabs and consoles: keys by separator read with SCAN, a value tab per type with TTL, a line console, read-only refusal | D (built; editing values next) |
| MongoDB: collections as table or JSON; queries | The `mongodb` driver program of a plug-in (ADR-0175): collections as a table with nested values as JSON or as documents in a JSON view, edited in the grid by `_id`, queries in the MongoDB shell's syntax; no tree of a document's fields | E (partly built, M8CECdRJ) |
| ClickHouse | Over HTTP: databases, tables, views, dictionaries, columns; streamed typed results; cancel by query id | D (built) |
| Elasticsearch, OpenSearch; Cassandra, ScyllaDB | Planned | E |

## Delivery order

1. **Stage A, the host**: the workspace host, its buttons, keys and restarts, and
   this empty Database workspace with SQL consoles, one front surface (A9), the Git workspace on
   the host (VVoJikso) and splits with scripts moving in and out (sXs4vGek). Built.
2. **Stage B, foundations**: the decisions (ADR-0163 to done), `zen-db` with its
   runtime, the PostgreSQL driver, the connection registry and its files and the connection
   lifecycle (built), and secrets in the Keychain.
3. **Stage C, daily work**: the connection dialog, the navigator, running statements in a console,
   results and the data editor with staged edits, transactions.
4. **Stage D, the second tier**: filters and Link with Editor, Advanced Copy and Paste, object
   editors and DDL, query history, sessions, SSH, Redis.
5. **Stage E, later**: data transfer, diagrams, the other non-relational databases.

## Out of scope

Left out on purpose: AI features, updates of their own, accounts and cloud sync, telemetry,
edition gating, separate editor, theme, Git and terminal settings (the IDE has them), a task
scheduler, dashboards, notebooks, the visual query builder, mock data generation, editing ER
diagrams, and federated queries across connections.
