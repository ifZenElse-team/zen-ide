# Database workspace

The Database workspace is a database tool inside the IDE window. The **Database** button on the
right tool window bar expands it over the editor area, where all your database work happens in tabs
of its own; collapse it to get back to your code, and expand it again to find every tab as you left
it. Today it holds the Database Navigator, SQL consoles that run on your connections with their
results below them, object tabs with a table's data, properties and DDL, the value panel, your
connections and the connection dialog, and filtering, sorting and editing the data.

## Opening and closing it

- Click **Database** at the top of the right tool window bar (the database icon, under the Git
  workspace's button), or choose **Database Workspace** in Find Action (⇧⌘A). The workspace slides
  in over the editor; your tool windows, the toolbar and the status bar stay where they are.
- While it is expanded, the editor's tab strip stays on top with a **Database** tab, active, after
  your code's tabs. Click a code tab to go back to that file, and the **Database** tab to come back.
- Click the button again, or press ⇧⎋, to collapse it. Opening any code file (from the Project tool
  window, Go to File, a link) collapses it too and shows the file. Only consoles and `.sql` files
  open as tabs of the workspace; any other file opens in the editor.
- When the workspace has more tabs than fit, the tab in front is always scrolled into view whole,
  and the row never starts with half a tab; ▾ at the end lists every tab.
  window, Go to File, a link) collapses it too and shows the file. The **Database** tab stays in
  the strip while the workspace is open; its × closes the workspace (your tabs and consoles are
  kept for the next time you open it).
- Your code tabs are untouched underneath: when the workspace collapses they come back with their
  carets and scrolling.

### Splitting the workspace

The workspace's tabs can sit side by side, as in the editor: a console beside a table's data, two
consoles one above the other.

- Right-click a tab and choose **Split Right** or **Split Down** (or **Split and Move Right**,
  **Split and Move Down**), or use the editor's Split Right and Split Down while the workspace is
  expanded. A console stays in its old place too unless you move it; any other tab (a table, Query
  History, Sessions) moves.
- Drag a tab onto another part of the workspace to move it there. Click into a part to give it the
  keyboard; **Goto Next Splitter** goes round the parts, and drag a divider to resize them.
- A part left without tabs goes away; **Unsplit** and **Unsplit All** in a tab's menu gather the
  tabs again.
- The results panel shows under the console that has the keyboard.
- The split is remembered with the workspace's tabs, also after a restart.

The workspace is part of the built-in **Database** plug-in: on from the start in GoGarden,
RustRonin, WebWave, SqlScroll and DevopsDaruma, switched on in the other IDEs in **Settings → Plugins → Built-in** (it works after a
restart). When it is off, the button is not shown.

## The Overview tab

The first tab, **Overview**, is always there and does not close. Until connections exist it says
**No connections yet** and offers **New Connection…**. Once you have connections,
it lists them: a dot in the colour of the connection type, the name, the driver and address, the
state, **Edit** and **Connect** or **Disconnect**. Long names and addresses end with "…", the states
and the buttons stay in line, and in a narrow window the state moves under the name; nothing
overlaps. Connecting runs in the background; the IDE never waits for a database.

### Databases your project names

The IDE looks through the project's `.env` files (`.env`, `.env.local`, `.env.example`…) and Docker
Compose files (`docker-compose.yml`, `compose.yaml` and their variants) for the databases they name:
`DATABASE_URL` and the other `…_URL` values, the `PG*`, `POSTGRES_*` and `MYSQL_*` settings, and
PostgreSQL, MySQL, MariaDB, Redis, Valkey, MongoDB, ClickHouse, Elasticsearch, OpenSearch, Cassandra
and ScyllaDB services with their published ports. It reads them in the background, four folders
deep, and skips excluded folders, `node_modules` and folders Git ignores.

What it finds is listed under **Detected in this project** on the Overview, and as links at the top
of the connection dialog: for example *db — PostgreSQL localhost:5433 / shop · docker-compose.yml*.

- **Add…** opens the connection dialog filled in. Nothing is saved and nothing connects until you
  press **OK**; a password found in the file is then stored in the macOS Keychain, never in a file.
  The connection remembers the file it came from.
- **Dismiss** stops offering it in this project, until the file names another database.
- **When the file changes** (another port, another database), the offer comes back as **Update…**,
  which changes the connection you added from it.
- An offer whose database the IDE has no driver for yet is shown with **Add…** turned off.

### Importing connections

**Import Connections…** on the Overview reads connections from another tool:

- **DBeaver**: its workspace folder (`~/Library/DBeaverData/workspace6` is filled in), a project
  folder in it, or its `data-sources.json`.
- **JetBrains project**: a project folder with `.idea/dataSources.xml` (the open project is filled
  in), with your user names from `dataSources.local.xml`.

**Find** lists the connections it holds; tick the ones to take, choose whether to keep them in this
project, and **Import**. Names, folders, hosts, ports, databases, user names, DBeaver's connection
types and SSH hosts come along; passwords do not: the other tool's password store is never read, so
you type each password once, at the first connect. Connections already here are not added twice,
those of databases the IDE has no driver for yet are listed and skipped, and the other tool's files
are never changed.

## Connections

PostgreSQL 12 to 18, MySQL 8.0 and later, MariaDB 10.6 and later, SQLite, Redis and Valkey, and
ClickHouse are built in: nothing to download.

### Compatible databases

Many databases speak the protocol of one of these, and the dialog lists them under it, each with
its own defaults:

- **PostgreSQL and compatible**: CockroachDB, YugabyteDB, TimescaleDB, Neon, Supabase, Amazon
  Redshift, Greenplum, AlloyDB, Citus, Materialize, RisingWave, QuestDB and CrateDB.
- **MySQL and compatible**: TiDB, SingleStore, PlanetScale, Vitess, StarRocks, Apache Doris,
  Percona Server and Amazon Aurora MySQL.
- **Redis and compatible**: Valkey, KeyDB, Dragonfly and Garnet.

Choosing one fills its usual port, database, user and SSL mode (CockroachDB: port 26257, database
`defaultdb`; Redshift: 5439 and `dev`, SSL required). A URL picks it too, by its scheme
(`cockroachdb://`, `redshift://`, `valkey://`, `valkeys://` with TLS) or by the host of the managed
service (`….neon.tech`, `….supabase.co`, `….redshift.amazonaws.com`, an Aurora cluster endpoint,
`….psdb.cloud`).

Some of them have less in their system catalogue than PostgreSQL. For CockroachDB, Redshift,
Materialize, RisingWave, QuestDB and CrateDB the navigator shows schemas, tables, views, their
columns and keys, and hides the folders those servers do not have (sequences, routines, triggers,
policies); TiDB, StarRocks and Doris hide routines, triggers and events.

**Test Connection** names the database it found: if a plain PostgreSQL connection turns out to be
CockroachDB, it says so and offers **Use the CockroachDB profile**. A saved connection that turns out
to be another database is read the way that database needs, and a notification offers to use its
profile; your settings change only when you choose that.

### Drivers from plugins

Other databases come as plugins that bring a **driver program**: install the plugin in Settings →
Plugins, and its databases appear in the connection dialog's list in every IDE, after the built-in
ones. The program is downloaded only when you install or update the plugin, for your computer only,
and checked against the checksum the signed plugin catalog names, before it is installed and before
every start; updating the plugin replaces the program when it changed. The team's plugins (SQL
Server, MongoDB, DuckDB) bring their program for macOS, Linux and Windows, signed on macOS;
the update the IDE makes by itself at start leaves such plugins for you to update. The IDE starts
the program when you connect and stops it when the last connection that uses it closes; a program
that stops on its own is started again when the connection is used next. Your password is handed
to the program when it connects and never written anywhere. Everything else (the navigator,
consoles, results, editing, export, history and production safety) works as far as the driver says
it can. Writing a driver: see the protocol in the developer documentation
(`docs/database-drivers.md`).

### SQL Server and Azure SQL

The **SQL Server** plugin brings Microsoft SQL Server and Azure SQL; it joins the plugin catalog with
its first release.

- **Connecting**: the host, the port (1433 by default), the database, and a SQL Server login with its
  password. For Microsoft Entra ID, choose **accessToken** under **authentication** and paste an
  access token as the password. Azure SQL's redirection to another server is followed.
- **TLS**: **Disable** connects without it (and says so when the server requires it), **Prefer** uses
  it when the server has it, **Require** always uses it without checking the certificate (the usual
  choice for a server's own self-signed certificate), **Verify CA** and **Verify Full** check it.
  The driver's **strict** setting is TDS 8.0, which SQL Server 2022 and Azure SQL offer: TLS before
  anything else is sent.
- **Navigator and completion**: databases, schemas, tables, views, procedures, functions and
  sequences, with columns, keys, foreign keys, checks, indexes, definitions and descriptions.
- **Running SQL**: consoles split scripts on `GO`; results are read page by page and **Cancel** stops
  the statement on the server. Manual commit, Commit and Rollback, and the isolation levels from
  READ UNCOMMITTED to SERIALIZABLE and SNAPSHOT work as for the built-in databases. An error points
  at the line the server names.
- **Editing data**: rows of a table with a primary key are edited in the results and saved in
  T-SQL, with values passed as parameters.
- **Not yet**: Windows logins, named instances (give the instance's port), `PRINT` messages, and the
  results after the first one of a batch.

### MongoDB

The **MongoDB** plugin brings MongoDB 3.6 and newer; it joins the plugin catalog with its first
release.

- **Connecting**: the host and port (27017 by default), or a `mongodb://` or `mongodb+srv://`
  address (Atlas gives one), the database, and a user with its password. Settings: **authSource**
  (the database that holds the user, `admin` by default), **authMechanism** (SCRAM-SHA-256 or
  SCRAM-SHA-1, by default the one the server offers; MONGODB-X509 logs in with the client
  certificate set under TLS, no password needed), **replicaSet**, **directConnection** (stay on
  the server named instead of going to the replica set's primary).
- **TLS**: **Disable**, **Prefer** (tries TLS, then connects without it), **Require** (no
  certificate check), **Verify CA** and **Verify Full**; a `mongodb+srv://` address and `tls=true`
  in an address turn it on.
- **Navigator**: databases, their collections and views, and each collection's indexes.
- **Consoles** take the MongoDB shell's statements: `db.orders.find({ total: { $gt: 10 } })`, with
  `.sort()`, `.limit()`, `.skip()` on the same or the next lines, `findOne`, `aggregate`,
  `countDocuments`, `distinct`, `insertOne`, `insertMany`, `updateOne`, `updateMany`, `replaceOne`,
  `deleteOne`, `deleteMany`, `createIndex`, `drop`, `db.runCommand({ … })`, `show dbs`,
  `show collections` and `use shop`. End a statement with `;` or a blank line. Values are written as
  in the shell: `ObjectId("…")`, `ISODate("…")`, `NumberDecimal("…")`, `/regex/i`.
- **Results**: each document is a row; its top-level fields are the columns, nested documents and
  arrays are shown as JSON. **JSON** at the bottom of the results shows the documents as the shell
  writes them, one after another (fields a document lacks are left out); click one to select it.
  Results are read page by page and **Cancel** stops the operation on the server. A read-only
  connection refuses statements that change data.
- **Editing documents**: the results of a `find` or `findOne` of one collection are edited in the
  grid as a table's rows are: change a field (typed as its BSON type: text, numbers, `true`/`false`,
  an ObjectId's 24 digits, JSON for documents and arrays), add, copy or delete documents, then
  **Save**. A document is found by its `_id` and the old values of the fields you changed, so one
  changed by someone else meanwhile is reported as a conflict; **Set to Default** removes a field. A
  new document gets a new ObjectId unless you give its `_id`. MongoDB saves each document on its
  own, without a transaction: when one fails, those before it stay saved. Opening a collection from
  the navigator opens a console with its `find`.
- **Transactions**: switch the console to manual commit and statements run in one transaction
  until **Commit** or **Rollback**. MongoDB has transactions on replica sets and sharded clusters
  (Atlas included), not on a single standalone server.
- **Not yet**: Kerberos, LDAP and AWS logins.

### DuckDB

The **DuckDB** plugin opens DuckDB database files and in-memory databases; it joins the plugin
catalog with its first release. It uses the `duckdb` program, version 1.0 or newer, which you
install from duckdb.org (or with Homebrew: `brew install duckdb`); an older one is refused when you
connect. The program runs without its start-up file (`~/.duckdbrc`) and never downloads an
extension by itself: `INSTALL` one when DuckDB asks for it.

- **Connecting**: **file** is the database file; leave it empty for an in-memory database.
  **program** is the `duckdb` program when it is not on your `PATH`. A read-only connection opens
  the file read-only. A new DuckDB connection needs no password. Without the `duckdb` program,
  connecting and running a query say that the program is missing.
- **Consoles** take DuckDB's SQL, one statement at a time; files are read with it too:
  `SELECT * FROM 'orders.parquet'`, `SELECT * FROM read_csv('orders.csv')`. Dot-commands of the
  `duckdb` program (`.tables`, `.shell`) are not run, on any line of a statement, and neither is a
  line with only `/` or `GO`.
- **AI agents** run DuckDB's SQL on the database itself only: a statement of an agent that reads or
  writes other files (`read_csv`, `FROM 'orders.parquet'`, `COPY`, `ATTACH`, `EXPORT DATABASE`),
  installs or loads an extension, changes extension settings or reaches other servers is refused
  with code `agentOutsideDatabase`, and its answer hands the statement to you in a new console.
- **Results** are typed as DuckDB types them: dates, decimals, lists and structs (shown as JSON),
  blobs. **Commit**, **Rollback** and manual commit work as for other databases.
- **Navigator**: databases (attached files too), schemas, tables, views and their columns; opening a
  table opens a console with its `SELECT`.
- **Cancel** stops the statement by ending the `duckdb` program; the next statement starts it again.
  A database file keeps its data, an in-memory database starts empty.
- **Not yet**: editing results.

### Oracle

The **Oracle** plugin connects to Oracle Database 12.2 and newer, Autonomous Database included; it
joins the plugin catalog with its first release. It uses **SQLcl**, Oracle's free command-line
program, which you install from oracle.com/sqlcl (or with Homebrew: `brew install --cask sqlcl`),
and SQLcl needs Java 17 or newer.

- **Finding SQLcl and Java**: SQLcl is found from the connection's **program** setting (the `sql`
  program or SQLcl's folder), then `SQLCL_HOME`, `ORACLE_HOME/sqlcl`, your `PATH` and Homebrew.
  **javaHome** is the JDK SQLcl runs on when it does not find one through `JAVA_HOME` or your
  `PATH`. Without one of them, connecting says "SQLcl is missing" or "Java is missing" and which
  setting to fill in. Both take absolute paths, and only on your own connections: a connection
  shared in the project never names a program to run, so there SQLcl is found where you installed it.
- **Connecting**: the host, the port (1521 by default), the user and the password, and under
  **connectionType** how the database is named: **serviceName** (the database field is the service
  name, `FREEPDB1` by default), **sid** (the database field is the SID), **tns** (an alias of
  `tnsnames.ora`, from the **tnsAlias** setting or the database field; **tnsAdmin** is the folder that
  holds the file) or **custom** (a connect string in **connectString**: Easy Connect such as
  `tcps://db.example.com:2484/sales?ssl_server_dn_match=true`, or a descriptor written without
  spaces). **role** connects as SYSDBA, SYSOPER, SYSDG, SYSBACKUP or SYSKM. The password is given to
  SQLcl privately, never on its command line.
- **TLS**: **Require**, **Verify CA** and **Verify Full** connect over TCPS; only Verify Full checks
  the host name. Java checks the server's certificate against its trust store, or against the
  wallet folder given in **wallet**. For Autonomous Database, set **wallet** to the wallet `.zip`
  you downloaded and choose a **tns** alias from it (`sales_high`).
- **Consoles** take Oracle SQL and PL/SQL. Scripts are split on `;`, except PL/SQL blocks and units
  (`DECLARE`, `BEGIN`, `CREATE OR REPLACE PROCEDURE`, `PACKAGE`, `TRIGGER`, `TYPE`), which end at a
  line with only `/`. Commands of SQLcl itself (`SET`, `SPOOL`, `HOST`, `@script`) are not run.
- **Results** are typed as Oracle types them: exact numbers, dates and timestamps, raw bytes. An
  error points at the place Oracle names, with its code (`ORA-00942`). Results are read up to the
  row limit at once, not page by page.
- **Navigator and completion**: schemas (Oracle's own ones as system schemas), with their tables,
  views, materialized views, packages, procedures, functions, sequences and types, with columns,
  keys, foreign keys, checks, indexes, comments and source.
- **Transactions**: manual commit, **Commit** and **Rollback**, and the isolation levels READ
  COMMITTED and SERIALIZABLE. Remember that Oracle commits every `CREATE`, `ALTER` and `DROP`.
- **Editing data**: the rows of a query of one table with a primary key (`SELECT * FROM hr.employees
  WHERE …`), and tables opened from the navigator, are edited in the results and saved in one
  transaction.
- **Cancel** asks SQLcl to stop the statement; if it does not stop within three seconds, SQLcl is
  ended and the next statement connects again (an open transaction is then rolled back).
- **Not yet**: `DBMS_OUTPUT` messages, execution plans, a CA file or client certificate from the TLS
  page (use a wallet), Kerberos and operating-system logins, triggers, synonyms and database links in
  the navigator, and Cancel that keeps the session on Windows.


- A MySQL server's databases are listed right under the connection, each with **Tables**,
  **Views**, **Functions**, **Procedures** and **Events**;
  a table also shows its columns, keys, foreign keys, indexes, triggers and the tables that refer to
  it. **Default schema** on the General page is the database a session starts in (`USE`).
- Connections use TCP even to `localhost`. The SSL mode `prefer` uses TLS when the server offers it;
  `require` uses it without checking the certificate; `verify-ca` and `verify-full` check it against
  the system's roots or the CA file, and both also check the host name for now, so a certificate
  made for another name is refused rather than accepted.
- Values are shown as the server writes them: exact decimals, dates and times with zero dates such
  as `0000-00-00`, `BIT(1)` as true or false, wider bits as numbers, JSON, enums and sets as text,
  binary strings as hexadecimal. Warnings of a statement appear in the Output with their code.
- **Cancel** stops a statement on the server (`KILL QUERY` sent on a second connection). A statement
  that only waits in `SLEEP()` is not an error to MySQL: it ends at once and shows its result.
- On a Production connection the IDE's sessions start with `max_execution_time` (MySQL) or
  `max_statement_time` (MariaDB) set to the type's statement timeout, and the navigator's own
  queries wait at most 10 seconds for a lock (`lock_wait_timeout`).
- DDL (**Generate SQL → DDL**) is the server's own `SHOW CREATE` statement as read with the
  structure; routines, triggers and events with a body come wrapped in `DELIMITER $$ … $$` so the
  text runs as a script.

### The connection dialog

**New Connection…** (on the Overview, on the workspace's toolbar, or **New Database Connection…** in
Find Action) opens one dialog: the drivers and the compatible databases on the left, grouped by
family, with a search field, and the chosen driver's settings beside, on four pages. **Edit** on the Overview opens the same dialog for a
connection.

- **Main**: connect by **Host** (host, port, database; **Show all databases**, on for PostgreSQL) or
  by **URL**. Paste a URI such as `postgresql://ann:secret@db.example.com:5432/shop?sslmode=require`
  or `mysql://root:secret@db.example.com:3306/shop?ssl-mode=REQUIRED` (`mariadb://` for MariaDB),
  or for a SQLite file `sqlite:///Users/ann/shop.db` (`sqlite:data/shop.db` for a relative path),
  in the URL field or straight into Host, and it fills every field; the password (also one given
  as `?password=`) moves into the password field and is never kept in the URL. A client key's
  `sslpassword` is not kept either, in the URL or in the settings. Then the authentication (user and password, or none),
  the user, the password and **Save password** (on). Redis, Valkey, SQLite and ClickHouse's
  `default` user start with no authentication (also from a URL without a password), so they connect without
  asking for one; choose **User & password** when the server wants one.
- **SSH/SSL**: an SSH host or `~/.ssh/config` alias, user, port and jump hosts; the SSL mode
  (`disable` to `verify-full`), a CA file, a client certificate and key, and direct TLS for
  PostgreSQL 17 and later. See *Through SSH* below.
- **Advanced**: the driver's own settings (for PostgreSQL, **Behind a transaction pooler**) and a
  table of other driver properties.
- **General**: the name, the connection type, the folder, whether it is kept for every project or in
  this project, read-only, the navigator's view, system objects and which schemas to show, and what
  happens on connecting: auto-commit, the isolation level, the default schema, keep-alive and
  bootstrap queries.

**Test Connection** tries the settings as they are in the dialog, in the background, without saving
anything: it shows the server's version and the round-trip time, or the error with its likely cause
(an unknown host, nothing listening on the port, TLS, the user name or password, no answer in time).
You can cancel it. **OK** saves the connection; for a new one, **Connect after saving** (on at first)
connects it right away, and when you clear it nothing connects until you connect it. A page longer
than the dialog scrolls, with its scroll bar shown.

- **Global or project.** A global connection is usable in every project. A project connection is
  kept in the project's `.zen/data-sources.json`, so your team shares it through Git; it never holds
  a password, and your user name and other personal settings stay on your computer.
- **Connection types.** Every connection has a type: **Development** (the default), **Test** (green;
  asks before saving data changes) or **Production** (red; changes are not committed until you
  commit, statements that change data ask first, and statements stop after 5 minutes). Types also
  end idle transactions and close idle connections after a while; you get a notification.
  Changing a connection's type, or a type's settings, applies to its open consoles at once: a
  connection made Production stops committing on its own without reconnecting. A console with
  uncommitted changes keeps manual commit until you commit or roll back, and a type change never
  commits anything. The question before a change says whether it will be committed at once or
  stay in the open transaction.
- **Removing a connection** closes its tabs (its tables and views, plans, sessions and keys, and the
  result tabs of its consoles); the confirmation says how many. Its consoles stay open with their
  text, showing **No connection: … was removed** until you choose another connection. Unsaved data
  or structure changes in those tabs are asked about first.
- **Passwords** are never written to a file: a saved password lives only in the macOS Keychain, and
  the IDE reads it once per session, when you first connect. A password you do not save is asked
  for at each connect and forgotten when you disconnect. Turning **Save password** off deletes the
  saved one.
- **While a connection connects**, the Overview says what it waits for: the Keychain, the SSH
  tunnel or the server; **Cancel** stops it. When macOS asks for access to the Keychain (a rebuilt
  IDE, a locked screen), connecting waits 30 seconds for the answer and then stops with a message
  that the Keychain did not answer: allow access in the system's prompt and connect again.
- **When the network fails**, the IDE reconnects by itself and restores the schema you chose. A
  query that only reads is run again once; a statement that changes data, or that calls a function
  that might (`SELECT archive_orders()`), never is: you are told it was not run again, so you can
  check.
- **Cancel** stops a query on the server within a second. A statement that changed data and
  finished while the cancel was on its way shows its result, because the change took effect.
- **Statements run after connecting** that change data are refused on a read-only connection, and
  the connection is read-only before they run.
- **After your Mac sleeps**, connections close and open again the next time you use them.
- **Behind pgbouncer** (transaction pooling), turn on the pooler setting: the IDE then never holds a
  transaction open.

## Through SSH

When the database is only reachable from a server you log in to, fill in the **SSH/SSL** page: the
server's host, or an alias from your `~/.ssh/config`, and if needed the user, the port and jump
hosts.

- **Your own SSH setup is used.** The IDE runs the system's `ssh`, so your config, keys, agent,
  `UseKeychain` and `ProxyJump` work exactly as in Terminal. The tunnel opens when you connect (and
  for **Test Connection**), and closes when you disconnect.
- **ssh's questions come to you.** A password or a key's passphrase is asked in a dialog; tick
  **Save in Keychain** to be asked only once. With jump hosts, the IDE's own or a `ProxyJump` in
  `~/.ssh/config`, the saved password is only for the SSH host itself (by its name or its
  `HostName`): a jump host's password is always asked. An unknown server key is shown to you and
  is never accepted on its own: answer **Yes** only after checking its fingerprint.
- **The database is reached only once ssh itself says the tunnel is open.** Another program that
  happens to listen on the tunnel's local port is never taken for it, so your database password is
  never sent there; if ssh stops or never says so, the connection fails instead.
- **When it fails** you see what ssh said, such as an unknown host name or "Permission denied".
- The connection to the database itself is still encrypted when its SSL mode says so, and its
  certificate is checked as without a tunnel: `verify-full` checks the database's real name, not the
  tunnel's, for PostgreSQL, MySQL, MariaDB, ClickHouse and Redis. A database of a driver program
  (MongoDB…) gets only the tunnel's address, so there `verify-full` and `verify-ca` refuse to connect
  through a tunnel rather than check the wrong name.

## SQLite

An SQLite connection opens a database file on your computer.

- **Main** has one field, **Path**: the file, or `:memory:` for a database that lives only while
  the connection is open. **Open…** picks a file; **Create…** makes a new, empty one and never
  replaces a file that exists. In a project connection a relative path starts at the project's
  folder, so your team can share it. Connecting never creates a file: a path that does not exist
  says so.
- **Opening a `.db` or `.sqlite` file** (also `.sqlite3`, `.db3`, `.s3db`) from the Project tool
  window offers it in the Database workspace: the connection dialog opens with the file filled in,
  or, when a connection already opens that file, the workspace expands and selects it. A file with
  such a name that is not an SQLite database opens in the editor as before.
- **Read-only** (on the General page) opens the file read-only, so nothing can change it.
- **The `sqlite3` program does the work.** macOS has it; on Linux it usually comes with the system
  or the `sqlite3` package, and on Windows install it from sqlite.org. When it is not on your `PATH`,
  set its path on the **Advanced** page. Version 3.37 or later is needed (macOS 13 and later have
  one); an older program is refused with its version. It is SQLite itself, so a file another program is using is
  as safe as with any SQLite client; when the file is locked, a statement waits up to 5 seconds.
- **The navigator** shows the file with its databases: `main` and `temp` when it holds something,
  each with its tables (columns, keys, foreign keys, indexes,
  triggers) and views. A change made by your SQL, or by another program, is picked up on refresh.
  The DDL tab shows the statement SQLite keeps for each object.
- **Values** come back as SQLite stores them: integers, reals, text, blobs as bytes, NULL; a
  column's type in the results is the storage class of its values.
- **Transactions**: in manual commit the IDE starts each transaction with `BEGIN` in the mode chosen
  as the isolation level (`deferred`, `immediate` or `exclusive`), also before a `SAVEPOINT`, so
  releasing a savepoint (as saving the grid does) never commits behind your back. In auto-commit a
  `SAVEPOINT` opens a transaction that releasing it commits, and the toolbar follows it. Every
  console of an SQLite connection shares its one session, and so its transaction.
- **Cancel** interrupts the running statement. The `sqlite3` program then restarts, so an open
  transaction is rolled back; the next statement, also one started right away, runs on the new
  program.
- An empty table opens on its columns and no rows, and so does a query that returns no rows.
- Only plain SQL runs: one statement at a time, never `sqlite3`'s dot-commands such as `.tables`,
  nor a line of only `/` or `GO`, which `sqlite3` reads as the end of a statement.
- **Nothing outside the file is touched.** `sqlite3` runs in its safe mode: `ATTACH`,
  `VACUUM INTO`, `readfile()`, `writefile()`, `load_extension()` and commands such as `.shell` are
  refused, also when a view or trigger of the file holds them.

## The Database Navigator

The panel on the left of the workspace is the **Database Navigator**: a tree of your connections,
in their folders, and of everything in them.

- **What it shows.** Each connection shows its address in grey, a dot when it is connected, a lock
  when it is read-only, a red mark when connecting failed, and a tint of its connection type's
  colour. Under it: the databases, their schemas, folders such as **Tables**, **Views**,
  **Functions**, **Sequences** and **Types**, and the objects. A table opens into **Columns** (each
  with its type), **Constraints**, **Foreign Keys**, **Indexes**, **Triggers** and **References**
  (other tables pointing at it). On the right of each table its size on disk shows as text (`72K`,
  `4M`) over a bar of how big it is next to the others; hover it for the size and the estimated
  rows. Long names end with "…", and in a narrow panel the size column gives way to the name.
- **PostgreSQL**: each schema lists **Foreign Tables**, **Materialized
  Views**, **Indexes** (all of the schema's indexes) and **Aggregate Functions** besides Tables,
  Views, Functions and Sequences; each table has **Policies** and **Partitions**; and the database
  lists **Extensions**, **Event Triggers**, **Tablespaces** and **Roles** after its schemas. Any of
  them opens on its Properties.
- **Expanding** a connection, double-clicking it or pressing ↩ on it connects it. What was read before shows at once, even before you
  connect and offline: the IDE keeps each connection's structure in its caches (never in your
  project, never any data). What it does not know yet is read in the background, with a
  **Loading…** row; you can keep working meanwhile.
- **It keeps up.** When you connect, and after you change the structure (create, alter, drop) in a
  console, the IDE checks which schemas changed and reads only those again. **Refresh** (F5 or ⌘R,
  or the toolbar's button) does the same on demand and keeps everything you expanded.
- **Opening an object** (double-click, ↩ or F4) opens it in its own tab on its **Properties**; a
  table's rows are on the next page, **Data**, and **View Data** in the menu opens
  them directly (see Object tabs below). F4 also opens a schema, and F4 on a connection edits it.
- **Selecting** an object from elsewhere (Search Everywhere, Link with Editor, an AI agent) scrolls
  the tree to it.
- **Changing objects.** **Delete…** (⌦ or ⌘⌫) drops a schema, a table, a view or another object, or
  a column, index or constraint of a table; **Tools ▸ Truncate…** deletes every row of a table and
  keeps it. Both first say what is lost (the table's rows and size, its columns, indexes and
  triggers, the foreign keys of other tables that point at it, a schema's objects) and show the
  statement; on PostgreSQL **Cascade** also drops what depends on it. **Rename…** (F2) renames a
  schema, an object or a column. These run at once, under the connection type's safety rules; a
  read-only connection refuses them. **Create ▸** (Table, View, Sequence, Function, Schema; on a
  table also Column and Index) opens the schema's or the table's tab with the form of the new
  object, whose script you see before it runs (see Changing structure below).
- **Connections.** **Duplicate Connection** copies a connection with its settings and saved
  passwords as "Name (copy)"; **Rename…** (F2) renames it and **Delete…** (⌦) removes it with its
  saved passwords, never touching the database.
- **Filter.** Type in the box at the top, or just start typing in the tree: only matching names stay,
  with the folders above them opened. ⎋ clears it.
- **Right-click** for Open Console, Edit Connection…, Duplicate Connection, Connect, Reconnect,
  Disconnect, View Data, Open Object, Create ▸, **Generate SQL** (the DDL, a SELECT or an INSERT,
  written into a new console), Tools ▸ Truncate…, Copy Name, Copy Qualified Name, Rename… (F2),
  Delete… (⌦) and Refresh.
- **Options** (⋮ on the panel's toolbar): the **Simple** view (tables, views and their columns) or
  **Advanced** (everything), system objects, the host, descriptions, sizes, connected
  connections only, and hiding the panel. **Link with Editor** selects the connection of the console
  you are in.
- The panel's width, what you expanded and selected, and the options come back after a restart.

## Redis and Valkey

A Redis connection takes a host and port, or a URI: `redis://user:password@host:6379/0`,
`rediss://…` for TLS, or `redis+sentinel://sentinel1:26379,sentinel2/primary/0` for a primary found
through Sentinel. On **Advanced**, **mode** chooses standalone, Sentinel or Cluster (list the
cluster's nodes as `host:port,host:port`), **separator** the character that groups keys into folders
(`:`), and **master** the Sentinel primary's name. Valkey works the same way.

- **In the navigator** a connection shows its logical databases (`db0`, `db2`… with their number of
  keys; in a cluster, its primaries instead), and each holds its keys grouped into folders by the
  separator: `user:1` and `user:2` are in **user**. Keys are read with `SCAN` a level at a time, never
  with `KEYS`, so a large database stays responsive; a level shows at most 10 000 keys and a **…** row
  says when more were left out. Each key shows its type.
- **Opening a key** (double-click, ↩ or F4) opens its value in a tab: the type, the time to live,
  the length, the memory it takes and its encoding, and below them a string as text (JSON laid out)
  or the items as a grid: a hash's fields, a list's elements with their index, a set's members, a
  sorted set's members with their scores, a stream's entries, a JSON document. **Set TTL** makes the
  key expire after the seconds you type, **Persist** makes it never expire, **Refresh** reads it again.
  The first 100 items show, with how many there are.
- **The console** takes one command per line, as `redis-cli` does, with quotes for spaces
  (`SET "my key" "a value"`); ⌘⏎ runs the line under the caret and the reply shows in the results:
  a hash as fields and values, a list numbered. Cancel stops a blocking command such as `BLPOP`.
  A database you choose with `SELECT 3` stays chosen when the connection opens again.
- **Read-only** connections refuse every command the server marks as writing, scripts included.

## ClickHouse

A ClickHouse connection talks to the server's HTTP interface (port 8123, or 8443 with TLS: choose
**https** on Advanced or an SSL mode from `require`). The navigator shows its databases (system ones
when system objects are on), then **Tables**, **Views** and **Dictionaries** with each table's engine,
rows and size, then the columns. Opening a table writes a `SELECT … LIMIT 200` into a console.
Results stream in as they arrive; Cancel stops the query on the server. `SET` and `USE` last for the
console's session. A read-only connection is read-only on the server, and the connection type's
statement timeout becomes ClickHouse's `max_execution_time`.

## SQL consoles

**New Console** (⌃], on the workspace's toolbar, or in Find Action) creates a console on the
connection selected in the Database Navigator, else on the connection of the tab in front, and opens
it as a tab: `console.sql`, then `console_1.sql`, `console_2.sql`… A console is a real
editor: typing, undo, Find (⌘F), multiple carets and everything else work as in any file.
A console needs a connection: with none yet, **New Console** opens **New Connection…** instead, as
its tooltip says. When two open consoles have the same file name (each connection's first console
is `console.sql`), their tabs add the connection's name: `console.sql (Shop)`, `console.sql
(Billing)`; a console alone keeps its plain name.

- **Highlighting**, **folding**, the **Structure** tool window and **syntax problems** come from the
  **SQL** plug-in, read in the console's connection's dialect (see [SQL files without a
  connection](#sql-files-without-a-connection)). It comes with GoGarden, RustRonin, WebWave, SqlScroll and DevopsDaruma; elsewhere, the
  console shows the usual banner offering to install it. Everything else on this page works without
  it.
- Consoles are kept in the IDE's data folder, never in your project, under
  `consoles/<project>/<connection>/`. Until connections exist, every console belongs to the
  connection folder `local`. A console in the folder of a connection that is no longer in the project
  has no connection: you choose one before it runs, it never runs on another connection by itself.
- Closing a console while it is empty deletes it, so trying the command costs nothing. A console
  with text stays and is saved after a second of quiet, even when autosave is off.
- Consoles appear in **Recent Files** (⌘E) and the Switcher (⌃⇥); picking one expands the workspace
  on it.
- ⌘W closes the active tab of the workspace, never a code tab hidden beneath.
- **Local History** keeps a console's versions as it does for project files: right-click its tab or
  choose **Show History** in its tab's menu to compare or get back an earlier text.

## Running SQL

A console runs on the connection shown in the workspace's toolbar: pick one with the selector
(⌃9) and, if you like, a schema (⌃0). With only one connection, a console uses it. Running
connects first when needed.

The toolbar keeps the connection, schema and transaction controls whatever tab is in front. On a
table or another tab they show the connection that tab is on (else the one selected in the
navigator) and its transaction; the selectors then only show it: open a console to choose.
Every button says what it does, with its shortcut, when you point at it.

Inside the workspace the keys are the workspace's own; the IDE's ⌘ keys of the same actions work
too, and outside the workspace every key keeps its usual meaning:

| Action | Key | Also |
|---|---|---|
| Execute statement | ⌘⏎ | |
| Execute script | ⌥X | ⌥⌘⏎ |
| Execute in a new result tab | ⌘\\ | ⌃⌘⏎ |
| Execute from the caret | ⌥P | |
| Explain plan | ⌃⇧E | ⌥⌘E |
| New console | ⌃] | |
| New connection | ⌃⇧N | |
| Format SQL | ⌃⇧F | ⌥⌘L |
| Comment line | ⌃/ | ⌘/ |
| Commit, Rollback | ⌃4, ⌃8 | ⌃⌘K, ⌃⌘Z |
| Choose connection, schema | ⌃9, ⌃0 | ⌥⌘9, ⌥⌘0 |
| Show or hide, maximise the results | ⌃T, ⌃⇧T | |
| Go to a database object | ⌃⇧D | ⌘O |
| Advanced Copy, Advanced Paste | ⌃⇧C, ⌃⇧V | ⇧⌘C, ⌥⇧⌘V |
| Focus the filter | ⌃⌥⇧T | ⌥⌘F |

Settings → Keymap → Database lists them all and lets you change any.

In a narrow window the toolbar keeps the run buttons, the connection, schema and transaction
controls in view: **Explain Plan**, **Execute on Several Connections…** and **Export From Query…**
move into the **⋯** (More Actions) menu, and New Connection and New Console show only their icon.
The other bars of the workspace do the same: the result's Fetch Next, Fetch All and Export… and the
object's structure actions past the edge go into **⋯**, the Plan tab's options and totals wrap to a
second row, Find in Database folds its column types into one menu, and the Sessions tab shows its
buttons as icons.

- **⌘⏎ runs the statement under the caret**, or the selection when you made one. The statement is
  found for you: it ends at a `;` or, by default, at a blank line (the caret right after a `;` still
  belongs to the statement before it). The statement under the caret has a light band behind it,
  and the statements that ran flash for a moment when the run ends (not with Reduce motion).
  Quotes, comments, PostgreSQL's `$$` bodies, `BEGIN … END` blocks, MySQL's `DELIMITER` and SQL
  Server's `GO` are understood; psql commands such as `\c` are not sent.
- **⌥X runs the whole script** (or the selection), statement after statement. An error stops it and
  rolls back the open transaction. **⌘\\** runs the statement into a new result tab.
- **Cancel** (⌘F2, the stop button, or Cancel in the header line) stops the statement on the server
  within a second. Long runs go on in the background; the console stays usable.
- After each statement the console shows how long it took and how many rows it read or changed. A
  failed statement is underlined where the server points, with its message.

### One script on several connections

To run the same script on several databases (say, every region or every environment), click
**Execute on Several Connections…** in the console's toolbar or in the bar above a `.sql` file.
Tick the connections in the order they should run; the console's own connection is ticked first.

- **They run one after another**, each in the console's session on that connection, connected when
  its turn comes. Statements are split in each connection's own dialect.
- **Stop at the first error** (on by default) stops the failing connection's script, rolls back its
  open transaction and skips the connections after it. Untick it to run everything and see every
  error.
- **Safety works per connection, before anything runs.** If any connection refuses (a read-only one
  and a script that changes something), nothing runs and the dialog says which connection refuses
  and why. If any connection asks first (Production, or a risky statement on any type), one
  confirmation lists **every** connection with its type and the statements that ask there; a risky
  one needs "I understand" ticked. AI agents follow each connection's type too: on Production they
  only read, and a change is handed to you.
- **Parameters and variables are asked once.** A script with `:name` placeholders or `${name}`
  variables opens the Bind Parameters dialog once for all the connections; the values are bound on
  each server, or written as quoted literals on a database without bind parameters (SQLite), which
  that connection's output says.
- **Results are grouped per connection.** The results panel gets a **Connections** tab with each
  connection's outcome: done, failed (with the error), skipped or cancelled, its statements, rows
  and time; a click opens its first result. Result tabs are named after their connection, such as
  "Orders EU · Result 1". **Cancel** (⌘F2) stops the running connection and the rest never start.
- The Query History marks these statements with the source **Several Connections**.

## Parameters and variables

SQL copied from code runs as it is: write `:name` placeholders (also `$1` on PostgreSQL and SQLite, `?` on
MySQL, MariaDB and SQLite, and `?1` on SQLite) and the **Bind Parameters** dialog asks their values before the
statement runs. The first value has the keyboard; ⇥ and ⇧⇥ move between the values. Choose each
value's type (Text, Number, Boolean or NULL); a number that does not read as one cannot run, nor can
an empty value: type one, or choose NULL. The values are sent to the server as real bind parameters, never pasted into the
text, so quotes in a value can never change the statement. SQLite and ClickHouse have no bind
parameters: there the values are written into the statement as safely quoted literals, and the
dialog and the Output tab say so (a negative value after a minus is written with a space, `5- -3`,
so it never turns into a comment). Placeholders inside strings, comments and quoted names are left
alone.

The console remembers the values you gave, also after a restart, and fills them in the next time;
values whose names look secret (`password`, `token`, `api_key`…) are kept only until the IDE
closes and shown hidden. The Bind Parameters dialog shows each time you run statements with
placeholders, so you always see what is sent. A new placeholder starts with the type its place
suggests: Number next to a number, in arithmetic or after `LIMIT` and `OFFSET`, the column's type
when it is compared with or assigned to a numeric or boolean column the Database Navigator knows,
the type it is cast to (`$1::int`), else Text. A value you gave before keeps its own type. A value
that does not read as its type is marked once you type it; an empty one only keeps **Run** off.
Explain Plan and Execute on Several Connections ask in the same dialog.

**Variables** are text put into statements before they run, such as a table or schema name:

```sql
@set table = orders
@include setup.sql
SELECT * FROM ${table} WHERE customer_id = :customer;
```

`@set name = value` sets a variable for the statements after it, and `@include file.sql` runs another
file's statements in its place (a file next to the script or in the project). The **Variables** tab
of the results panel lists the console's variables, where you can change, add and remove them, and
the parameter values it remembers, which you can forget.
## SQL files of your project

Any `.sql` file of your project can run against a database too. Open it in the editor: a bar above
it shows **Execute** (⌘⏎), **Execute Script** (⌥⌘⏎), the **connection** the file runs on and its
**schema**.

- **Bind the file** by choosing a connection, and a schema if you like, in that bar (or with the
  workspace's selectors when the file is open there). The choice is remembered for this file in this
  project, outside the file and outside the repository, and comes back after a restart.
- **A file is never run on a guess.** Until you choose a connection, ⌘⏎, Execute and the run icons
  open the connection list first; the run goes on once you pick one, and nothing runs if you close it.
- **Every statement has a run icon** in the gutter, on its first line. A click runs that statement,
  also when it follows the one before it on the same line without a space.
- **Results open in the Database workspace**: the file opens there as a tab, with its results panel
  below, as a console's. Collapse the workspace to go back to your code.
- Completion, ⌘B, F1 and the warnings work in a bound file as in a console, and statements are split
  in the connection's dialect (a Redis connection runs one command per line).
- In the Database Navigator, **SQL Editor** on a connection lists its **Scripts** (the bound files
  and the consoles) and makes a **New Script…** in a folder of your project that you choose.
- **Move a script between the editor and the workspace** by dragging its tab: onto the **Database**
  tab in the editor's tab strip (or, while the workspace is expanded, into the workspace) to work on
  it there, and back onto the editor's tab strip to return it to your code; **Move to Editor** in its
  tab's menu does the same. Consoles stay in the workspace: they live in the IDE's data folder, not
  in your project. A code file other than `.sql` does not go into the workspace, and the IDE says
  why.

## SQL files without a connection

The **SQL** plug-in helps with every `.sql` file, also one that is not bound to a connection and
in an IDE where the Database workspace is switched off:

- **Folding** of statements, bracketed lists and subqueries, `BEGIN … END` and `CASE … END`,
  comments, function bodies and `-- region Name` … `-- endregion` (⌘−, ⌘=, or the gutter).
- **Structure** (⌘7, ⌘F12): every statement, the tables, views, functions and other objects the
  script creates by their name, a table's columns under it, and a `WITH` query's parts.
- **Completion** (⌃Space, or as you type) of the SQL keywords, data types and built-in functions,
  in the case you started typing; what usually comes next first (`BY` after `ORDER`, a type after a
  column name in `CREATE TABLE`).
- **Syntax problems** as you type: a string, quoted name or comment left open, brackets that do not
  match, `CASE` without `END`, a comma before `FROM` or `)`.

In a console, or a file bound to a connection, all of it follows the connection's database:
MySQL's `#` comments and `\'` in strings, PostgreSQL's `$$` function bodies, SQLite's `[names]`.
Without a connection, only what is wrong in every common dialect is underlined. Completion in a
bound file comes from the connection (below), with its tables and columns.

## Completion and navigation in SQL

A console knows its connection's structure, so it completes and navigates offline, at once, from the
structure the IDE keeps for the connection. Nothing connects by itself for it.

- **Completion** comes as you type a name or a `.`, or with ⌃Space; ↩ or Tab accepts. It offers what
  fits where the caret is: at the start of a statement its keywords; after `FROM`, `JOIN`, `UPDATE`
  or `INSERT INTO` the tables and views of the console's schema, the other schemas, and the tables of
  other schemas written with their schema; after `alias.` that table's columns; in `SELECT`,
  `WHERE`, `ORDER BY` and the like the columns of every table the statement reads, then functions and
  keywords. Aliases, `WITH` queries and subqueries are understood, also in a statement you are still
  writing. A column two tables share is offered with its alias (`o.id`, `c.id`).
- **Joins from foreign keys.** After `JOIN` the tables linked to the ones already in the statement
  come first with the whole condition (`customers c ON c.id = o.customer_id`); after `ON`, the
  condition itself.
- Keywords are put in as you started typing them (`sel` gives `select`), and the case of what you
  typed is never changed while you type.
- **⌘B, ⌘-click or F4** on a table, view, column or schema opens it in its tab (a column on its
  table's Columns). **F1** shows its kind, comment and columns; with ⌘ held, the pointer over a name
  shows its columns in one line.
- **Unknown tables and columns** are underlined as warnings once the structure is loaded, only where
  the IDE is sure: in schemas it has read in full, never for tables the script creates.
- When a large schema was read by names only, the columns of the tables you use are read in the
  background while the connection is connected; the list fills in when they come.
- In a **Redis** console the first word completes the commands, with their arguments shown beside
  them, and a key argument the keys the navigator has already read.

## SQL in your code

SQL you write inside strings of a Go, Rust, Python, JavaScript, TypeScript, Java or Kotlin file is
treated as SQL, as in a console:

```go
rows, err := db.Query("SELECT id, total FROM orders WHERE customer_id = $1", id)
```

- **Which strings.** A string whose text reads as a statement: `SELECT …` in capitals, `select … from`,
  `INSERT INTO`, `UPDATE … SET`, `DELETE FROM`, `WITH … AS`, `CREATE TABLE` and the like; a template
  with the `sql` tag (`` sql`select …` ``); and any string after a `// language=SQL` comment on the line
  before it or on its own line (`# language=SQL` in Python). A comment naming another language
  (`// language=JSON`) keeps a string out. Words such as "Select a file" stay plain strings.
- **Highlighting.** Keywords, functions, literals, parameters (`$1`, `?`, `:name`) and comments take
  the SQL colours of your theme inside the quotes, with the SQL plug-in's colours when it is
  installed. It works in every IDE and costs nothing in files without such strings; typing looks
  again only at the lines you change.
- **Completion, navigation and warnings** come from a connection's structure, as in a console:
  tables, columns, joins, ⌘B and F1 on a name, and unknown tables and columns underlined. The
  connection is the project's default or one chosen for the file; until there is one, completion
  offers SQL keywords. Choose it with ⌥⏎ → **Choose SQL Connection…** in such a string (tick
  **For every file of the project** to make it the project's default). The list starts on the
  connection in use; ↑ and ↓ move however fast you press them, and ⏎ takes the row; agents use
  `db.injection.bind` and `db.injection.setDefault`. It is kept in the IDE's data folder, never in
  your files. Nothing connects for it.
- **Running it.** Put the caret in the string and press **⌘⏎**, click the run icon in the gutter
  of its line, or choose ⌥⏎ → **Run SQL**. The Database workspace opens with your file and the
  results below it (the only way a code file shows there: it otherwise takes `.sql` files only); times and errors are marked on the code, after the string, and the editor
  keeps its scroll. Several statements in one string: ⌘⏎
  runs the one under the caret, and a selection inside the string runs just that. Placeholders
  (`$1`, `?`, `:name`) open **Bind Parameters** first, and a Production connection asks before
  changes, as in a console. The first run without a connection asks for one right at the caret
  and then goes on.
- Escapes are read (`\"` is a quote to SQL), and completion writes them back (`\"Name\"` inside a
  double-quoted Go string); raw strings are taken as they are.
- **Not yet:** a query built from pieces (`"SELECT …" + where`) is read piece by piece, and the
  `${…}` parts of a template are read as written.

## Find Usages of tables and columns

**Find Usages** (⌥F7) shows where a table, view, function, procedure or column is used. Use it on a
row of the Database Navigator (or its context menu), with the **Find Usages** button of an object
tab's Properties page (a column selected in **Columns** looks for that column), or on a name in a
console, an SQL file or SQL inside a code string.

- **Where it looks.** The project's `.sql` files and the SQL strings of Go, Rust, Python,
  JavaScript, TypeScript, Java and Kotlin files that belong to this connection (or to no
  connection), the connection's consoles, and the database's own views, functions, procedures,
  triggers and foreign keys as the IDE last read them.
- **Only real usages.** Names are read as ⌘B reads them: aliases, `WITH` queries, schemas and quoted
  names count, so `orders` in another schema, a `WITH orders AS …` or a comment is left out. Each
  usage is marked as a read or a write (the table of an `INSERT`, `UPDATE` or `DELETE`, a column it
  sets).
- **The results** open in the **Find** tool window, grouped by file, then by the connection and the
  object that holds them; opening a usage in the database opens that object's tab. The header counts
  the usages in files apart from those in the database ("2 usages in 2 files and 1 in the
  database"). The search runs in the background and can be stopped.
- **Rename.** Renaming a table, view, routine or column on its Properties page (**Rename…** or **Edit
  Column**) keeps **Find usages in the project to rename** ticked: the usages open in the Find tool
  window as a rename, and **Replace All** or **Replace Selected** writes the new name into your
  files. Usages in consoles and in the database are listed but never replaced; the database changes
  only when you save the staged structure change.
- Nothing connects for it: it works from the structure the IDE keeps for the connection, so connect
  once or refresh the connection in the navigator first.

## Formatting SQL

**Code → Reformat Code** (⌥⌘L, or ⌃⇧F in the Database workspace) formats a console or an SQL file (`.sql`, `.pgsql`, `.mysql` and the
like) in the dialect of its connection: PostgreSQL's dollar-quoted bodies and `E''` strings,
MySQL's backticks, `#` comments and backslash escapes, SQL Server's brackets. With a selection, only
the statements it touches are formatted; otherwise the whole file, as one step that ⌘Z undoes. It
also works in the project view's menu and, with **Actions on Save → Reformat code**, on saving.

- Each clause (`SELECT`, `FROM`, each `JOIN`, `WHERE`, `GROUP BY`, `ORDER BY`, `LIMIT`, `UNION`,
  `VALUES`, `SET`, `RETURNING`) starts a line; the items of a list and each `AND` and `OR` of a
  condition go on lines of their own, indented; subqueries, `WITH` queries and the columns of
  `CREATE TABLE` are indented inside their parentheses. Calls, `IN` lists and `CASE` stay on one line.
- **Settings → Editor → Code Style → SQL** sets the indentation, the keyword case (as typed by
  default), where commas go, whether a clause keyword stands alone on its line, and whether list
  items get a line each.
- **Nothing changes meaning.** Only spaces, line breaks and the case of reserved keywords change:
  strings, comments, quoted names and every other word stay exactly as written, and the result is
  checked against the original before it is used. A statement with an unclosed quote or comment, or
  unmatched brackets, is left as it is and a notice says so. psql commands (`\d`) and `DELIMITER`
  lines are never touched, and procedural code (`BEGIN … END` bodies) only has its keyword case
  changed. Formatting twice gives the same text.
- Formatting runs in the background and connects nowhere.

## Transactions

Each console has its own transaction. The workspace's toolbar shows how it commits and what is
waiting to be committed.

- **The mode** (Auto, Manual or Smart) is the toolbar's one commit-mode control; its tooltip says
  what the mode does and its menu has the isolation level. **Auto**
  commits every statement; **Manual** waits for **Commit** (⌃4) or **Rollback** (⌃8); **Smart**
  commits on its own until a statement changes data (anything not starting with SELECT, SHOW, USE,
  SET or EXPLAIN), then waits for you, and goes back to auto if that statement fails. Production
  connections start in Manual; smart commit is off unless you choose it.
- **The counter** next to Commit and Rollback shows how many statements the open transaction
  holds: 0 in grey when none is open, then green, orange from 5, red from 20. Point at it to see the
  statements that changed data and how long it has been open; click it for the transaction log, with
  Commit and Rollback.
- **A console tab with an open transaction** has a dot on its icon. Changes saved from the grid
  inside an open transaction count among its statements.
- **Commit and Rollback tell the truth.** When a statement failed inside a PostgreSQL transaction,
  the server rolls it back on Commit: the IDE says so instead of reporting a commit. A MySQL DDL
  statement commits the open transaction before it runs, so the toolbar shows it ended even when
  the DDL failed. A schema or read-only switch made inside a transaction stays after Rollback.
  Commit and Rollback first close a result that is still reading, so they never wait behind it.
- **Changing only the isolation level** keeps the open transaction; the new level applies to the
  next one.
- **Nothing is lost by accident.** Closing a console, disconnecting, reconnecting or switching a
  console to another connection while changes are not committed asks **Commit**, **Rollback** or
  **Cancel**. A transaction that only read is simply rolled back.
- **Idle transactions.** When a transaction has been idle as long as its connection type allows
  (Development 30 min, Test 15, Production 10), a countdown gives you 10 seconds to keep it open;
  then it is rolled back and a notification says so.
- **Pending Transactions** (in the mode menu or Find Action) lists the open transactions of every
  connection, with Commit and Rollback for each.

## Production safety

Mistakes on a production database are hard to make, and AI agents follow the same rules.

- **You always see where you are.** A console of a connection whose type confirms SQL execution
  (Production) has a frame in the type's colour, and the toolbar shows the connection type next to
  the connection. Tabs carry the colour too.
- **Confirm before changes.** On Production, every statement that changes data or structure asks
  first, one at a time: **Yes** runs it, **Skip** leaves it out (offered only when there are several
  statements), **Yes to All** runs the rest, **Cancel** runs nothing. Reads, `SET` and `BEGIN`/`COMMIT` never ask. Running the same thing again
  is not an answer, and neither is Enter or ⌘⏎ while the question is open.
- **Risky statements ask on every connection**, Development too: `DELETE` or `UPDATE` without a
  `WHERE` (or with one that is always true, such as `WHERE 1 = 1`), also inside a `WITH`, `DROP`
  (also `ALTER TABLE … DROP COLUMN`), `TRUNCATE`, and `FLUSHALL`/`FLUSHDB` on Redis. The question
  names the risk and the table, and **Run Anyway** works only after you tick "I understand".
- **Read-only connections.** The lock in the toolbar (or **Read-only** in the connection dialog, or a
  read-only connection type) makes a connection refuse every change: statements
  that change data or structure, saving edited rows, changing Redis keys. Where the database has a
  read-only session (PostgreSQL, MySQL, MariaDB, SQLite, ClickHouse, Redis), the IDE uses it too, so
  even a function that writes inside a `SELECT` is stopped by the server. The navigator shows a lock
  on read-only connections.
- **AI agents** follow the connection type. On Development and Test they may do what you do, and
  confirm a risky statement explicitly. On **Production they only read**: a change, saving edits or
  committing is refused with the reason, and the agent can open the SQL in a console for you to
  review and run. A type can also forbid agents everything: then agents see nothing of its
  connections at all — no data, no table or column names, no structure and none of your consoles or
  history — only that the connection exists. Agents can make a connection safer
  (lock it), never less safe. Otherwise agents read and run your consoles as they do the
  project's `.sql` files.
- **Everything is in the Query History**: statements that were refused or skipped (orange,
  marked **Refused**), statements you confirmed (marked **Confirmed**), and what agents ran (marked
  **Agent**).

## Explain Plan

**Explain Plan** (⌃⇧E, or the plan button in the console toolbar) shows how the database will run
the selection or the statement under the caret, without running it. The plan opens in a **Plan** tab
of the workspace, one per console; ⌃⇧E on a Plan tab explains its statement again.

- **The tree** lists every step of the plan with its operation (Seq Scan, Hash Join, Index lookup),
  the table or index it reads, its cost, the estimated rows (and the real rows after a slash when
  analyzed), its time and its condition. **Hot nodes** — the steps that take the largest share of the
  time (or, without ANALYZE, of the cost) — are tinted, the hottest strongest, with a bar under the
  time or cost; the hottest is chosen first. Click a node to see everything the database said about
  it in the pane on the right. **Text** shows the plan as the database sent it.
- **Options** (PostgreSQL): **ANALYZE**, **VERBOSE**, **COSTS** (on by default), **BUFFERS** and
  **SETTINGS**; change them in the tab's bar and click **Explain**. MySQL and MariaDB explain in JSON
  and take ANALYZE; SQLite shows its query plan; other databases say they cannot explain.
- **ANALYZE runs the statement.** It is off by default and warns while it is on. It runs inside a
  transaction that is rolled back afterwards (a savepoint inside your own open transaction), so
  rows it changes are put back; what a transaction cannot undo, such as sequences, stays. It is
  checked like running the statement: refused on read-only connections, asked first on Production
  and for risky statements, and agents on Production cannot analyze changes. On MySQL and MariaDB,
  which keep changes to tables that are not transactional and commit structure changes at once,
  and on a database without transactions, ANALYZE explains only statements that read. Explain one
  statement at a time.
- **Parameters and variables** work as when you run the statement: `:name`, `$1` or `?`
  placeholders and `${name}` variables open the Bind Parameters dialog, and the values are bound on
  the server (PostgreSQL, MySQL, MariaDB), so the plan is the one the database makes for those
  values. SQLite gets them as quoted literals, and the Plan tab says so.

## Results

The results panel opens under the console; drag its top edge to resize it.

- **The header line** sums up the run: statements, rows changed and read, execution and fetch
  time. No statistics tab takes the focus.
- **A tab per result**, named after the table it reads or *Result N*, numbered from 1 again in each
  run: *orders*, then *orders 2* when the run reads it twice. A new run replaces the tabs you did not
  pin, and pinned tabs keep their names; right-click a tab to pin, rename or close it. **Output** shows what the server
- **A tab per result**, named after the table it reads and the run's number (`orders 3`, `orders(+) 4`
  when it joins others) or *Result N*. A new run replaces the tabs
  you did not pin; right-click a tab to pin, rename or close it. **Output** shows what the server
  said (NOTICE, RAISE, warnings), **Execution Log** every statement run, and **Variables** the
  console's variables and remembered parameter values. A statement that never ran (skipped
  after an error, cancelled before it started) has no time in the log.
- **The grid** scrolls through any number of rows. `[NULL]` is grey, numbers are right-aligned and
  booleans are check boxes; long values are cut, the whole value shows when you point at it. Drag a
  console's variables and remembered parameter values.
- **The grid** scrolls through any number of rows. `[NULL]` is grey, numbers are right-aligned
  (very large and very small floating-point numbers in exponent form, such as `1e300`) and booleans
  are check boxes; long values end in …, the whole value shows when you point at it. Each column
  header has a badge of its type (`123`, `abc`, `dt`, `{}`, `t/f`, `01`, `[]`); in SQLite it follows
  the type the table declares. Drag a
  column's edge to resize it, double-click it to fit. Click a cell, a row number or a header; drag
  or ⇧-click for more; ⌘C copies as tab-separated text (see [Copy and paste](#copy-and-paste)).
  **Text** shows the rows as aligned text and **Record** one row as a form you can filter; ⇥ switches
  between the grid and the record.
- **Column headers** show a key on primary key columns and a link on foreign key columns (the
  tooltip names the table). The **▾** beside a header's sort arrows opens the column's menu: sort
  ascending or descending, filter, select the column, fit its width, copy its name.
- **The context menu** is grouped: Copy, **Advanced Copy ▸**, Paste, **Advanced
  Paste ▸**, **Edit ▸**, **Filter ▸**, **Order ▸**, **Navigate ▸** (the referenced row, references,
  **Go to Row…** ⌃G, first and last row, Fetch Next, Fetch All), **View/Format ▸** (Dense Rows),
  **Panels ▸** (Value Panel), **Layout ▸** (Grid, Text, Record), Generate SQL ▸ and Export Data…;
  each item shows its shortcut.
- **Reading rows.** The first 200 rows show at once and reading goes on to 10 000; scrolling to
  the end reads the next page. **Fetch All** reads every row after asking. **200+** tells there are
  more: click it or **Calculate Total** to count them. The box after Export is the **fetch size**,
  the rows read at first and by each Fetch Next. The arrows at the left of the bottom bar go to the
  first row, a page back, a page forward (reading the next page when needed) and the last row read. Outside a transaction the IDE never leaves a lock on your tables:
  it commits its reading when done; inside your own transaction it reads as you scroll. All results
  of a window share 256 MB; past that, reading stops and says so.
- **Rows past 10 000 come from running the statement again**, so the IDE does that only for a query
  that just reads. For a statement that may change data, such as `UPDATE … RETURNING`, a `DELETE`
  with `RETURNING` or Redis `LPOP`, Fetch Next and Fetch All say that more rows need running it
  again, and nothing runs; run it again in the console if you mean to.
- Dates and times show as the server writes them, `timestamptz` in the session's time zone.

## Object tabs

A table, view, function, sequence, type or schema opens in a tab of its own, with a coloured line
of its connection type. A double click in the navigator opens it on **Properties**. Under the tab are its pages:

- **Data** (tables and views; a table opens on it): the rows in the same grid as console results,
  in the order the database keeps them (no `ORDER BY`; sort from a header when you
  need an order). They are read only when the connection is connected; otherwise
- **Data** (tables and views; the page after Properties, or straight away with **View Data** in the
  navigator's menu): the rows in the same grid as console results,
  ordered by the table's primary key. They are read only when the connection is connected; otherwise
  click **Read Data**, which connects. Refresh in the bottom bar reads them again.
- **Properties**: the name, comment, owner and tablespace, and on the left the sections — Columns,
  Constraints, Foreign Keys, Indexes, Triggers, References (tables pointing at this one),
  Dependencies (the views using it, the sequences it owns; for a view, what its query uses), Rules,
  Policies, Partitions, Statistics and Permissions for a table; the query of a view, the source of a function,
  the settings of a sequence, the objects of a schema. They come from the structure the IDE keeps,
  so they show offline too.
  Constraints, Foreign Keys, Indexes, Triggers, References (tables pointing at this one), Policies,
  Partitions, Statistics and Permissions for a table, each with how many entries it has; the query
  of a view, the source of a function, the settings of a sequence, the objects of a schema. What a
  database does not have is not shown: SQLite has no owner, tablespace, policies, partitions or
  permissions, MySQL and MariaDB no owner or policies. The rows are numbered once, at the left, and
  a long last column (an index's definition) ends with "…" in view; point at a cell for all of it.
  They come from the structure the IDE keeps, so they show offline too.
- **DDL**: the statements that create the object, with toggles for nested objects, permissions,
  separate foreign keys and key indexes, comments and partitions; **Copy** and **Open in Console**.
- **Diagram**: the table's ER diagram, with the tables it references and the tables that reference
  it (see [ER diagrams](#er-diagrams)).

The page and section you chose come back after a restart, without connecting and without any data.

## ER diagrams

Right-click a schema in the Database Navigator and choose **View Diagram** to see its tables and
views as an ER diagram in a tab of its own; on a table or view, **View Diagram** opens its
**Diagram** page. Each box lists the columns with their types; a key marks primary key columns and a
key with an arrow foreign key columns. Lines join a foreign key to the table it references: the
crow's foot is the referencing side, the bar the referenced one, and a small circle means the
reference may be empty. Views have a dashed outline; tables of other schemas are drawn fainter.

The diagram is read-only and comes from the structure the IDE keeps, so it shows offline and never
connects. The boxes are placed automatically, referenced tables to the left; a large schema is laid
out in the background (Cancel stops it), and the diagram follows changes to the structure.

- Drag the background or scroll to move around; ⌘ with the wheel, or a pinch, zooms.
- **−** and **+** in the bar zoom out and in; the button between them shows the zoom and goes back
  to 100% when clicked; **Fit** (the frame icon) shows the whole diagram.
- Click a box to select it and its lines; double-click it to open its object tab.
- **Export…** saves the diagram as a PNG image or an SVG image in the current theme's colours, or as
  Mermaid text; **Copy as Mermaid** copies the text.

The zoom and position come back after a restart.

### Changing structure

The **Properties** page also changes the object. A bar above it offers what fits the section you
are in: **Add Column**, **Edit Column** and **Drop Column** on Columns (select a row first),
**Add Constraint** (unique, primary key or check), **Add Foreign Key**, **Add Index** and their
drops, and for every object **Rename…**, **Comment…** and **Delete…** (with **Cascade** on
PostgreSQL). A schema's tab offers **New Table…**, **New View…**, **New Sequence…**, **New
Function…** (starting from a template you complete), **New Schema…**, and renaming, commenting and
deleting the schema. Only what the server can do is offered.

Nothing runs while you edit: each change is added to a list under the bar, checked at once against
what the server can do (a column that does not exist, a sequence on MySQL, a comment on SQLite are
refused with the reason), and the tab is marked as changed. Remove one change with its **×**, or all
with **Discard**. **Save…** shows the script exactly as it will run, in the connection's own SQL:

- **Execute** runs it. On PostgreSQL and SQLite it runs in one transaction, so an error leaves
  nothing half done; MySQL and MariaDB commit each statement on their own, and the window says so.
  Dropping a column, a constraint or an object asks first on every connection type, a Production
  connection asks before anything, and a read-only connection refuses. When it has run, the
  navigator and the tab show the new structure, read again for just the objects that changed; a
  renamed object's tab follows it, a deleted object's tab closes. If it fails, your changes stay in
  the list with the server's error.
- **Copy** copies the script; **Open in Console** opens it in a new console to edit and run by
  hand.
- **Save as Migration** writes it to a new file in your project, `migrations/<time
  stamp>_<what changed>.sql` (UTC), for your migration tool; an existing file is never written over.
- **Cancel** keeps the changes in the list.

SQLite cannot change a column's type, nullability or default, or add and drop constraints, in place:
the IDE rebuilds the table (a new table, the rows copied, the old one dropped, the new one renamed,
its indexes and triggers created again) and says so in the preview. Foreign keys are switched off
for the rebuild, so that no row referring to the table is deleted or changed, and switched on again
afterwards when they were on; for that it runs only in auto-commit with no transaction open. The
script runs whole: its confirmation has no **Skip**. When the table has something the
rebuild would lose — a `CHECK`, a generated column, `AUTOINCREMENT`, `COLLATE`, `STRICT` or
`WITHOUT ROWID` — it refuses; make that change in a console. On MySQL a column is restated whole, so a
column with `ON UPDATE` or `INVISIBLE`, or a generated column (other than renamed), is changed in a
console too.

Closing a tab with changes that were not run asks whether to discard them.

## The value panel

The value panel is open beside every grid at first; **Value Panel** in the bottom bar
of a result (⇧⌘P in the Database workspace) hides or shows it beside the grid, and the IDE remembers
your choice. It shows with the selected cell: as **Text** (arrays as `{1,"a b",NULL}`), **JSON** (indented, with
arrows to fold objects and arrays), **XML**, **Hex** or, for pictures stored as bytes, **Image**.
The format you pick is used whenever a value offers it. **Copy** copies what is shown, **Save…**
writes the value to a file. Under it, **Calc** shows the count, distinct values, sum, average,
minimum and maximum of the cells you selected; with more than one cell selected, the status bar
shows them too. In the **Record** view each field shows whole; click one to see it in the panel.

Spatial values — PostGIS `geometry` and `geography`, MySQL geometry, GeoPackage blobs, WKT text
such as `POLYGON((0 0, 4 0, 4 4, 0 0))`, and GeoJSON — open in **Geometry**: the panel draws them as
shapes on a plain canvas with a coordinate grid, and the caption tells the kind, the SRID and how
many points there are. Select several cells to see all their shapes together; the current cell is
drawn in the accent colour. Scroll to zoom, drag to move and double-click to fit. No map is loaded
from the internet. **Copy** and **Save…** give the value as WKT.

## Filtering and sorting

Above every grid, the **WHERE** box takes a condition as you would write it after `WHERE`, such as
`total > 10 AND name LIKE 'A%'`; press ↩ to apply it. While you type, column names and keywords are
offered (⌃Space shows them; ↩ or ⇥ takes one). The rows are read again in the background, and the
rows shown stay until the new ones come, while **Filtering…** and **Cancel** beside the box show it
running (Cancel stops it). Beside the box you see the statement the rows were read with. When a
filter fails, the box turns red and the error shows in place of the rows.

- **‹** and **›** go back and forward through the filters you used; **▾** lists the table's recent
  filters and your saved ones, and **Save Filter…** names the current one; **✕** clears the filter
  and the sort. ⌥⌘F puts the keyboard in the box.
- For a table, the condition goes into the table's query; for a console result, a simple `SELECT`
  gets it added to its `WHERE`, and any other query is filtered around it. A filter is always one
  condition: a `;` or unmatched parentheses or quotes are refused, so a filter can never run another
  statement.
- Right-click a cell, **Filter ▸**: equal to, not equal to, greater or less than its value, contains
  it, is or is not NULL, one of the selected rows' values, the clipboard's value, a value you type,
  one of the column's values (**By Values…**), or **Reset Column Filter**.
- The arrows at the right of a column header sort by it: click to go ascending, descending, unsorted;
  ⇧-click to sort by several columns. When all rows are loaded the IDE sorts them itself; otherwise
  they are read again in that order.
- Only a query that just reads is read again: the result of a statement that may change data (an
  `UPDATE` or `DELETE` with `RETURNING`) is never run again by a filter, a sort or **✕**, which say
  so instead.
  they are read again in that order. In a column holding values of several types (SQLite), numbers
  come first, then text and other values, then NULL.

## Editing data

When the rows of a result come from one table with a key, you can change them in PostgreSQL, MySQL,
MariaDB and SQLite (a read-only connection, a query joining tables, a table without a key or a
ClickHouse table says why not in the bar under the grid, after a **Read-only** lock; trying to
edit there turns the reason red, and the edit items of the context menu are disabled with the reason
as their tooltip; in SQLite, a console result is editable
when its query is a plain `SELECT` of one table;
for a table without a key, **Choose Key…** lets you pick the columns that identify a row, kept in
the project, or use all columns).

- Double-click a cell or press ↩ or F2 to edit it, or just start typing to replace its value; ↩
  keeps the value, Esc leaves it. **Set to NULL** is disabled on a `NOT NULL` column. Right-click for
  **Set to NULL**, **Set to Default**, **Copy from Row Above**, **Add Row** (⌘N), **Duplicate Row**
  (⌘D) and **Delete Rows** (⌘⌫). Changed cells, new rows and rows to delete are coloured, and the tab
  shows it has changes.
- While the rows are being read again (a filter, a sort or Refresh), they cannot be changed: wait
  until the new rows come. Filtering, sorting or refreshing rows with changes, running a statement
  that would replace an unpinned result tab with changes, or closing a result tab, a table's tab or
  a console with changes asks one question first (closing a tab: "Save them before closing it?"),
  answered **Save**, **Discard** or **Cancel**.
- Nothing is written until you **Save** (⌘S). **Preview…** shows the SQL that will run; on Test and
  Production connections Save always shows it first and asks you to confirm. **Cancel** (⌥⌘Z) drops
  the changes.
- Save writes everything in one transaction, so either all changes are saved or none. If someone
  else changed or deleted a row you edited, nothing is saved and the bar tells you which row. In
  manual commit mode the changes stay in your open transaction until you commit it, and count as
  changes not committed, so disconnecting or switching to Auto asks first.
- The SQL the IDE writes (saves, filters, lookups, imports, exports, structure and compare scripts)
  quotes every schema, table and column name, so a column named `to`, `key` or `current_date` works
  like any other.
- **Generate SQL ▸** in the menu copies SELECT, INSERT, UPDATE or DELETE statements of the selected
  rows.

## Following foreign keys

When the rows of a result come from one table, the columns of its foreign keys are links: their
values are drawn in the link colour.

- **Open the referenced row**: click the arrow that shows at the right of a key cell when you point
  at it, ⌘-click the cell, press ⌘B, or choose **Open Referenced Row** in its menu. The referenced
  table opens in its own tab, filtered to the row the key names (every column of a key of several
  columns). A key holding NULL points at no row.
- **References ▸** in a cell's menu lists the other tables whose foreign keys point at this table;
  choosing one opens that table filtered to the rows that point at the selected row.
- **Back** and **Forward** (the arrows at the left of the bar under the grid) return to the tab you
  came from and back again, each with the filter it had.
- **Choose a key from the referenced table** while editing: select a key cell and press ⇧↩, or choose
  **Choose Value…** in its menu. A list of the referenced table's rows opens (up to 200, the key
  first); type to search the key and text columns, click a row to put its key into the cell (all
  columns of a composite key). As with any edit, nothing is written until you Save.

## Query history

Every statement the IDE runs on a database is kept in the **Query History** tab: what you run in
consoles (⌘⏎, ⌥⌘⏎), the rows a filter or sort reads again, the rows of an object tab, saved data
edits, **Commit** and **Rollback**, the queries exports read, and the SQL AI agents run (marked
**Agent**). Open it with the clock button in the workspace's toolbar, or **Query History** in Find
Action.

- Each line shows when the statement started, the connection, what ran it, the statement, its time
  and the rows it read or changed; a dot tells how it ended (green done, red failed, orange
  cancelled).
- Type in **Search statements** to find statements holding all your words, in any case (the error
  message is searched too). Narrow the list by connection, type, result and date (today, the last 7
  or 30 days).
- Click a statement to see it whole, with its error. **Open in Console** (or a double-click) puts it
  in a new console of its connection, **Run Again** also runs it there, **Copy** copies it. Run Again
  refuses a statement whose connection is no longer in the project, and one that was cut or had a
  password masked: open it in a console and complete it first.
- The history is kept in the IDE's data folder (`databases/history.jsonl`), never in your project,
  and only statements: no rows, no values of parameters. Passwords written in statements
  (`PASSWORD '…'`, `IDENTIFIED BY …`, a URL's password, Redis `AUTH`) are replaced with `***`.
- ⚙ in the tab turns recording off and on, sets how many days (90) and how many statements
  (10 000) are kept, and clears the history. Clearing everything deletes the history's file.

## After a crash

While you work, the IDE keeps what a crash must not lose in a journal in its data folder
(`databases/recovery-….jsonl`, one per running IDE, readable by you only; an IDE and a headless
one for agents never mix theirs): the text of consoles you have not saved yet,
data edits you have staged and not saved (each changed row with the values it was read with and
its key), and the transactions you have open. Every record has a checksum, and the journal is
written to the disk at least once a second, so a crash, a forced quit or a power loss loses at
most the last second. It never holds passwords, and it is deleted as soon as there is nothing in
it.

If the IDE stopped without closing, the next start:

- puts a console's unsaved text back into its tab as unsaved changes (⌘Z returns to the text that
  was saved), and tells you so;
- shows a notification with **Review**, which opens the **Recovered** tab of the Database
  workspace. Each unsaved set of data changes is listed with its table and connection; click it to
  see the SQL that would save it: an update finds its row by the old key and checks the values
  read are still there. **Open SQL in Console** puts that SQL in a new console of the connection
  for you to read and run yourself. A transaction that was open is listed as rolled back: the
  server undid it when the connection ended, and its last statements are shown. **Discard** and
  **Discard All** drop what you no longer need, after you confirm: discarded work cannot be
  brought back.

Nothing is connected to or run against a database while recovering. On a normal exit, open
transactions are not reported again.

## Finding objects

Press ⌘O in the Database workspace (**Go to Database Object**), or open **Search Everywhere** (⇧
twice) and its **Database** tab, and type a name. Tables, views, functions, sequences, types,
schemas and columns of every connection are found at once, each with its kind, schema and
connection, from the structure the IDE keeps: offline too, and without connecting. Camel humps work
(`cuid` finds `customer_id`), and a dot narrows the search: `orders.id` finds the column `id` of
`orders`, `public.cust` the tables of `public`. Your scripts and consoles are found by file name and
the query history by the words of its statements. ↩ opens the object's tab (a column opens its
table's Properties), the script, or the statement in a new console, expanding the workspace when it
is collapsed. The All tab lists a few database matches too.

## Sessions and locks

**Sessions** in a connection's menu in the Database Navigator opens a tab with the server's
sessions of that connection. PostgreSQL, MySQL and MariaDB have
it; other databases say they do not.

- PostgreSQL shows each session's **Pid**, **User**, **Database**, **Application**, **Query start**,
  **State** and **Query**; MySQL and MariaDB show **Id**, **User**, **Host**, **Database**,
  **Command**, **Time**, **State** and **Query**. The IDE's own sessions are marked **IDE**.
- A session waiting for a lock has an orange dot, a session blocking others a red one. Below the
  list, the chosen session's query is shown whole with what it waits for and what it blocks, and
  **Locks** shows each blocking chain (`4243 ← 4242 ← 4241`: 4243 waits for 4242, which waits for
  4241) and the locks waited for with the locks that block them.
- **Show idle** (on) also lists idle sessions; a session idle inside a transaction, or one blocking
  others, is always listed. **Refresh** reads the sessions again; **Auto-refresh** reads them every
  2, 5, 10 or 30 seconds, only while the tab is in front and the connection is connected. The tab
  never connects by itself: a disconnected connection shows **Connect**.
- **Cancel Query** (MySQL: **Kill Query**) stops the chosen session's statement and keeps the
  session; **Terminate** (MySQL: **Kill Session**) ends the session and rolls back its open
  transaction. Both are also in a row's context menu, and both ask first, naming the session and
  what it runs; ending a session needs **I understand** ticked. They touch only the chosen session,
  are refused on a read-only connection, and go to the Query History (type **Sessions**). Stopping
  one of the IDE's own sessions says so in the question.
- Queries are shown as the server reports them, with passwords written in them replaced by `***`.

## Migrations

**Migrations** in a connection's menu in the Database Navigator opens a tab with the migration
folders of your project and, for the chosen folder, where each migration stands on the chosen
connection. The IDE recognises the folders from their files, in the background and without
connecting anywhere: Flyway (`V1__name.sql`), Liquibase changelogs, golang-migrate
(`1_name.up.sql` and `.down.sql`), goose (`-- +goose Up`), Atlas (`atlas.sum`), sqlx (a Rust
project using sqlx), Diesel (`<version>_name/up.sql`), Alembic (`versions/*.py`), Django
(`<app>/migrations/0001_name.py`), Prisma (`<time>_name/migration.sql`), Rails (`db/migrate`) and
Knex (a `knexfile`). Dependency, build, excluded and ignored folders are skipped, and the list
follows your edits.

- The two lists at the top choose the folder (shown with its tool) and the connection. Each
  migration has its **State**, **Version**, **Name** and **File**. In a narrow tab the columns
  shrink and a cut value ends with "…"; hover over a file to see its whole path. When the tab is too narrow for **Refresh**, **Up**
  and **Down** with their names, only their icons show, named in their tooltips.
- While the connection is connected, the tool's own table (`flyway_schema_history`,
  `schema_migrations`, `_sqlx_migrations`, `alembic_version`…) is read in the background:
  **Applied** (green), **Pending** (orange), **Failed** (red: failed or left dirty) and **No file**
  (applied on the database but missing in the project). Disconnected, every migration is
  **Unknown** and the tab offers **Connect**; it never connects by itself. **Refresh** looks for
  folders and reads the table again.
- **Up** applies every pending migration and **Down** reverts the last applied one, each with the
  project's own tool (`migrate`, `goose`, `flyway`, `sqlx`, `diesel`, `alembic`, `manage.py`,
  `prisma migrate deploy`, `rails`, `knex`…) as a run configuration in the Run window, where you see
  its output and can stop it. The tool gets the connection's host, port, database and user; the
  password is read from the Keychain first and given to the tool in a file only you can read,
  removed when it ends — never in its environment or on its command line. Tools that take a password
  only in a URL (Alembic, Django, Prisma, Rails, Knex, and MySQL with golang-migrate, goose, Atlas,
  sqlx or Diesel) are not given one: run those in the terminal with your own configuration. The
  tool is taken from your `PATH` outside the project, never from the project's own folders
  (`.venv`, `node_modules`, `bin`). Prisma cannot revert, so **Down** is off for it.
- Connect first: what **Up** would apply is read from the migrations the database has, so a run on
  a closed connection is refused with an offer to connect. Start runs from this tab; a saved run
  configuration or Rerun is sent back here, where the checks and questions are.
- **Down** always asks first, and so does **Up** on a connection whose type asks before changes
  (Production): the question shows the command and its risks. A read-only connection runs
  nothing: **Up** and **Down** are off there, and their tooltip tells why. Connections through SSH and databases other than PostgreSQL, MySQL, MariaDB and SQLite
  are not supported, since the tool cannot reach them.
- When a run ends, the connection's structure is read again in the Database Navigator, and the
  tab reads which migrations are applied.

## Command-line clients and code

- **Open in Terminal** in a connection's menu in the Database Navigator starts its own command-line
  client — psql, mysql or mariadb, sqlite3, redis-cli, clickhouse client — in a new tab of the
  Terminal tool window, with the connection's host, port, user, database and TLS settings. The
  password goes to the client privately, in a file only you can read that is removed when the
  client ends: it is never on the command line, in the client's environment or on the screen, and
  when you leave the client the tab becomes an ordinary shell without it. redis-cli asks you for the
  password instead. A read-only connection opens a read-only client session where the client has one. With no saved password
  the client asks for it. A connection through SSH needs to be connected first; the client then
  goes through the same tunnel. The client is looked for on your `PATH`, then in the folders
  installers use, since an IDE started from the Dock has a short `PATH`: `/opt/homebrew/bin`,
  `/usr/local/bin`, Homebrew's `libpq` and `mysql-client`, Postgres.app, `/usr/local/mysql/bin`,
  MacPorts' `/opt/local/bin`, `/usr/bin` and `/bin`. A folder of your project is never used. If the
  client is not installed (for example `brew install libpq` for psql), the IDE says which one it
  looked for.
- **Copy Client Command** copies the same command as one line for your own terminal, without the
  password.
- **Generate Code ▸ Go Struct, Rust Struct, TypeScript Interface** in a table's or view's menu
  copies a type with a field for each column: nullable columns become pointers, `Option` or
  `| null`, and column comments become doc comments.

## Find in Database

**Find in Database** (the magnifier in the workspace's toolbar, or **Find in Database…** on a
connection or a schema in the navigator) looks for a text in the tables of a schema, on the server:

- Type the text, choose the connection and the schema, and press **Enter** or **Find**. By default
  the text columns are searched, case-insensitively; tick **Numbers**, **Dates and times** or
  **Other** to search more kinds of columns, **Match case** to match letters exactly, **Names** to
  also find tables and columns whose names hold the text, and **Views** to search views too (each
  view runs its query). **Only columns** limits the search to columns with those names.
- Tables are searched one at a time in a read-only session of the search's own, so nothing is
  changed or locked. Each table stops at **Rows per table** (100) and **Seconds per table** (10): a
  slow table is stopped on the server and the search goes on. **Stop** cancels the search.
- On a Production connection, or a schema with more than a million rows, a warning says that the
  search loads the server.
- The list shows each table with the text: how many rows have it (100+ when there may be more) and
  in which columns. Double-click a table (or **Open Rows**) to open it filtered to those rows;
  right-click to keep only one column's rows. The filter goes into the table's filter history, and
  **Back** returns to the whole table.
- The search reads only whether a value has the text, never the values themselves; its statements
  are in the query history.

## Copy and paste

- **⌘C** copies the selected cells as tab-separated text, one row a line, so it pastes into
  Excel, Numbers or Google Sheets; a value with a tab, a quote or a line break is put in quotes.
  Right-click for **Copy with Header**, **Copy Column Names**, **Copy Row as JSON** and **Copy As ▸**:
  CSV, TSV, JSON, Markdown, HTML, XML, SQL INSERT, SQL UPDATE (each row by its key), SQL IN (…)
  (a condition you can put in a WHERE) and code for Go, Rust, TypeScript, Python and Kotlin. SQL is
  written in the connection's own dialect, with your unsaved edits included.
- **Advanced Copy…** (⇧⌘C) asks first: column names, row numbers, the column and row delimiters,
  the quote and when to use it, what NULL is written as, and whether values are copied as shown,
  as SQL literals or as JSON. Your choice is remembered.
- A large selection is copied in the background: the bar under the grid says so and has **Cancel
  Copy**. A copy holds at most 100 000 rows and 64 MiB; when more is selected the bar says how many
  rows were copied. Use **Export…** to take every row.
- **⌘V** in a table you can edit pastes rows copied from a spreadsheet or from a grid over the cells
  from the selected one; rows past the last become new rows. Tabs, commas or semicolons and quoted
  values (with line breaks inside) are recognised, and a first line naming the columns is left out;
  then each value goes into the column its header names, whatever their order.
  Every value is checked against its column's type first. If one does not fit, **Advanced Paste**
  opens with a preview of the rows and the problems (the line and the column); **Paste Valid Cells**
  pastes the rest. **Advanced Paste…** (⌥⇧⌘V) opens it anyway, to choose the delimiter, the header,
  what NULL is read from, and whether rows go over the cells or are added as new rows (also
  **Paste as New Rows** in the menu).
- Pasted values are staged like your other edits: nothing reaches the database until you save.
  While a save runs, its changes can be neither reverted nor restored.

## Exporting data

**Export…** in the bar under any result, **Export Data…** on a table, a view, a schema or a
**Tables** folder in the navigator, and **Export From Query** in a console's toolbar open one
**Export Data** dialog:

- **Format**: CSV (delimiter, quote, header, quote all values, what NULL is written as, formulas
  escaped so spreadsheets do not run them), TSV, JSON (an array, or one object per line), SQL
  `INSERT` statements in the connection's dialect (the target table and rows per statement),
  Markdown, HTML and XML; LF or CRLF line endings; UTF-8, with a BOM if you want one. **Parquet**
  keeps each column's type (numbers, exact decimals, dates and times, bytes) and is compressed with
  Snappy, gzip, LZ4 or not at all; you set the rows of a row group, which is all that is held in
  memory while it is written. Parquet goes only to files.
- **What**: the dialog names the table and the connection it exports. With several cells selected
  in the grid, **Selected cells only** exports just those rows and columns as they are shown, your
  unsaved edits included, without reading them again (any format but Parquet).
- **Output**: a file in a folder (**Browse…** picks it), named by a pattern (`${table}_${timestamp}` at first; also
  `${connection}`, `${date}` and `${index}`), or the clipboard. From a schema or a folder you choose
  the tables, and each goes to its own file. If a file exists, the dialog asks before replacing it.
- The rows are read again from the server and written as they arrive, so a table of millions of
  rows is exported without filling the memory. A result is exported with its filter and sort; a
  console's query runs in the console's session, so it sees your open transaction; tables are read
  in a read-only session of their own. On PostgreSQL, CSV and TSV are written by the server itself
  (`COPY`) when the settings allow, which is fastest; the option shows only for PostgreSQL.
- An export runs only a statement that just reads. Exporting the result of a statement that may
  change data, or **Export From Query** on one (such as `DELETE FROM orders` under the caret), is
  refused before anything runs; export the rows already loaded with **Selected cells only**, or run
  the statement in a console.
- While it runs the dialog shows the rows and bytes written; **Cancel Export** stops the query on the
  server and leaves no half-written file (files are written under a temporary name and moved into
  place when complete), and **Run in Background** closes the dialog while it goes on. A notification
  says when it is done, with **Open** and **Reveal in Finder**.
- The dialog remembers your format, settings, the last folder and pattern for next time.

## Backup and restore

**Tools → Backup…** and **Tools → Restore…** on a PostgreSQL, MySQL or MariaDB connection (and
Backup on a schema or a table, which narrows it to them) in the navigator open a dialog that runs
the database's own tools: `pg_dump`, `pg_restore` and `psql`, or `mysqldump` and `mysql` (MariaDB's
`mariadb-dump` and `mariadb`).

- **No setup.** The tools are looked for on your `PATH`, in Homebrew's folders (also keg-only
  formulae such as `libpq` and `mysql-client`, which are not on the `PATH`), in Postgres.app, and,
  when the database runs in Docker on this Mac, inside its container (the one that publishes the
  connection's port). **Tool** shows the one found with its version; the newest is used, unless the
  container's is newer (a `pg_dump` older than the server refuses to work). Choose another in the
  list if you want. Nothing found? The dialog says how to install it (`brew install libpq`).
- **Options per tool.** For `pg_dump`: the format (Custom, Plain SQL, Directory, tar), schema and
  data or one of them, schemas and tables, compression, encoding, parallel jobs for the directory
  format, `INSERT` instead of `COPY`, Clean (DROP) with IF EXISTS, Create database, no owner, no
  privileges. For `mysqldump`: schema and/or data, tables, a single transaction or locked tables,
  DROP TABLE, routines, events, triggers, extended or complete INSERTs, BLOBs as hex, comments. For
  restoring: what to restore, Clean, Create database, Single transaction, Stop on error and jobs on
  PostgreSQL (a plain SQL backup goes through `psql`, the others through `pg_restore`); Go on after
  errors on MySQL. **Extra arguments** are passed as they are.
- **It runs in the Run tool window**, where you see the tool's command line and its output, and
  stop it with ■. **Rerun** there runs the same job again with the same tool and options (a backup
  replaces its file; a restore asks again in the dialog, as does a password you typed for one run).
  A backup is written under a hidden name and moved into place only when complete,
  so a stopped or failed backup leaves nothing; the dialog asks before replacing a file.
- **A restore always asks first**: it changes the database. A read-only connection refuses it.
- **The right version**: for PostgreSQL the IDE asks the server its version and uses a client of
  the same version when you have one (Homebrew's `postgresql@NN`, Postgres.app, the database's
  container). A restore with only a newer `pg_restore`, or a backup with only an older `pg_dump`,
  is refused before anything runs, with the command that installs a matching client.
- **Your password stays out of sight**: the saved one (or the one you type for this run) goes to the
  tool in a file only you can read, removed when the run ends — never in its environment or on its
  command line. Tools in your project's own folders are never picked up from `PATH`.
- Connections through SSH are not backed up yet.

## Importing data from CSV

**Import Data…** on a table in the navigator imports a CSV file into it; on a schema or its
**Tables** folder it imports into a new table. The **Import from CSV** dialog has five steps:

1. **File**: a UTF-8 CSV file (type its path or **Browse…**), and the name of the new table when you
   create one.
2. **Format**: the delimiter (`\t` for a tab), the quote, the escape character (empty when quotes
   are written doubled), whether the first row holds the column names, what NULL is written as,
   and the date format (`dd.MM.yyyy HH:mm` or `%d.%m.%Y %H:%M`; empty for ISO 8601). The first 100
   rows show below with what each column seems to hold, and change as you change the format.
3. **Columns**: which table column each file column goes into (by name at first; **Skip** leaves it
   out) and how it is read: text, integer, decimal, boolean, date or date and time. For a new table
   you name the columns and set their types.
4. **Loading**: rows per statement, commit every N rows (0 imports the whole file in one
   transaction), stop and roll back or skip the row when a row fails, **Before loading** delete every
   row of the table, and `COPY` on PostgreSQL.
5. **Confirm**: what will happen; **Before loading** repeats that the table is emptied first.

The file is read row by row while it is imported, so files of any size work. On PostgreSQL the rows
go through `COPY … FROM STDIN`, elsewhere through `INSERT`s. When a row fails, the import finds it
and notes its row and line number; with **Skip** it goes on, with **Stop** what was not committed is
rolled back. The dialog shows how much of the file is read and the rows imported and skipped;
**Cancel Import** stops it and rolls back what was not committed, **Run in Background** closes the
dialog. A notification says how it ended, with the first rows that failed, and the table's tab,
when it is open and has no unsaved changes, reads its rows again. A read-only connection
refuses imports, and a Production connection, or emptying the table first, asks before anything
runs. The dialog remembers the format and the loading settings.

## Transferring data between databases

**Transfer Data…** on a table, a schema or its **Tables** folder in the navigator copies tables into
another connection, or into another schema of the same one, even between different servers
(PostgreSQL to MySQL, SQLite to ClickHouse…). The **Transfer Data** dialog has four steps:

1. **Target**: the tables to copy (from a schema you choose them), the target connection and its
   schema. Read-only connections are marked **(read-only)** and never chosen for you; if every other
   connection is read-only, you choose the target yourself.
2. **Tables and Columns**: each table's target name, marked **existing** when the target has a
   table of that name and **new** when it will be created. A column goes into the target column of
   the same name; you can rename it or leave it out, and for a new table change the type it gets.
   Types are written in the target's dialect (`character varying(40)` becomes `VARCHAR(40)` on
   MySQL); a new table gets the source's primary key.
3. **Loading**: rows per statement, commit every N rows (0 copies each table in one transaction),
   stop and roll back the table or skip the row when a row fails, **Before loading** delete every
   row of existing target tables, and `COPY` on PostgreSQL. When the settings of the last transfer
   are used, the step says so, and **Reset to defaults** brings back the defaults.
4. **Confirm**: what will happen; **Before loading** repeats that tables are emptied first.

The dialog keeps one size through its steps.

Each table is read page by page in a read-only session of its own while its rows are written, so
tables of any size are copied without filling the memory. Values are converted for the target's
types; a value that does not fit (a word into a number column) is noted with its row and column.
The dialog shows the table being copied and the rows read, written and skipped; **Cancel Transfer**
stops both servers and rolls back what the current table has not committed (on MySQL, MariaDB and
ClickHouse a created table and an emptied one stay so: their structure statements commit on their
own), **Run in Background**
closes the dialog. A notification says how it ended. A read-only target refuses, a Production
target, or emptying tables first, asks before anything runs, and the dialog remembers the loading
settings. A table is never copied into itself (choose another schema or name), and one transfer
takes at most 200 tables.

## Comparing schemas and data

**Compare** in the workspace's toolbar opens the **Compare** tab. Choose a **Source** and a
**Target**: a connection and its schema, or a snapshot you saved before. **Compare** reads both
structures again (connecting if needed) and lists what differs as a tree: objects only in the
source (+), only in the target (−) and changed (~), each with the columns, keys, indexes and
triggers that differ. Choose an object to see its DDL from both sides next to each other. The tab
always shows the window's latest comparison, with its source and target chosen, also one an AI agent
started with `db.compare.schemas` or `db.compare.data`.

- **Open Script in Console** writes the statements that make the target like the source, in the
  target's SQL, into a new console of the target's connection. Nothing runs until you run it there,
  and Production connections ask first. **Copy Script** puts it on the clipboard. What only the
  target has is dropped only when **Include drops** is ticked; otherwise the script says so in a
  comment, as it does for what the database cannot change in place.
- **Save Source Snapshot** keeps the source schema's structure (never its data) in a file, so you
  can compare with it later, for example before and after a migration.
- On a changed table, **Compare Data** compares its rows by primary key: how many are the same,
  changed, only in the source and only in the target, the differing rows, and **Open Data Script in
  Console** with the `INSERT` and `UPDATE` statements (and `DELETE` with **Include drops**). Rows
  are read a page at a time, so no lock is held while it runs, and **Cancel** stops it.

## After a restart

The workspace comes back as you left it: expanded or collapsed, its tabs in order, each console
with its caret and scroll position. Nothing connects to a database by itself: every connection
comes back disconnected until you connect it.

## For AI agents

Every action is a command (see [AI agents](11-ai-agents.md)). `view.dataSource.open` opens the
connection dialog, filled in from its parameters (`{ id?, driver?, profile?, url?, name?, host?, … }`), and
`db.driver.list` lists the drivers with the fields their connections have (a plugin's driver also
says its plugin and whether its program is installed) and the compatible databases (`profiles`;
`query`, `family`, `limit`), and `db.driver.log { id, limit? }` shows a plugin driver's log without
passwords. `db.console.new { connection?, text? }`
creates and opens a console, `db.console.list { connection?, limit? }` lists the project's consoles,
`db.console.execute`, `.executeScript` and `db.query.run { connection, sql, parameters?, variables? }` run SQL
(placeholders are bound from `parameters`; a run without them is refused with code `parametersNeeded`),
`db.console.parameters`, `.variables`, `.setVariable` and `.setParameter` list and set a console's
parameters and variables,
`db.script.bind { path?, connection?, schema? }`, `.state`, `.run { path?, scope?, offset? }`, `.targets`,
`.list { connection? }` and `.new { connection, folder?, name? }` bind, run, list and create the `.sql`
files of the project (an unbound file is refused with code `noBinding` and the connections to choose),
`db.script.runOnConnections { path?, connections, stopOnError?, parameters?, variables? }` runs a console's script on several
connections in turn and `db.script.connectionsState { path?, waitMs? }` reads each one's outcome,
`db.export.start` exports rows to files or the clipboard (`db.export.state`, `db.export.cancel`),
`db.csvImport.preview` and `db.csvImport.start` import a CSV file into a table (`db.csvImport.state`,
`db.csvImport.cancel`),
`db.search.start { connection, text, schema? }` finds a text in a schema's tables (`db.search.state`,
`.cancel`, `.openResult`, `.targets`, `.open`),
`db.compare.schemas` and `db.compare.data` compare schemas and rows (`db.compare.state`, `.ddl`, `.script`),
`db.results.list` and `db.result.page { id, offset, limit }` read results page by page,
`db.result.filter`, `.sort`, `.filterByValue` and `.filterState` filter and sort them,
`db.result.setCell`, `.addRow`, `.deleteRows`, `.previewChanges`, `.save` and `.revert` edit them,
`db.result.relations`, `.openReferenced`, `.openReferencing`, `db.data.back`, `.forward` and
`db.lookup.start`, `.state`, `.choose`, `.close` follow foreign keys and choose a key while editing,
`db.dataSource.add`, `.list`, `.get`, `.update`, `.remove` and `.test` manage connections,
`db.databaseFile.create { path }` makes an empty SQLite file and `db.databaseFile.open { path }` opens
one as the Project tool window does,
`db.connectionType.*` the connection types, and `db.connection.connect`, `.disconnect`,
`.disconnectAll`, `.invalidate`, `.status` and `.cancel` connect and disconnect them, with
`db.operation.state` telling how each went; `db.ssh.prompts` and `db.ssh.answer` answer ssh's
questions; `db.detect.list`, `.accept` and `.dismiss` list, add and dismiss the databases the
project's files name (without their passwords; for an agent, offers from files hidden from agents
come without values and cannot be added by it), and `db.import.dbeaver` and `db.import.jetbrains`
import connections (`preview` lists them first); `db.connection.readOnly { connection, readOnly? }`
locks a connection (an agent can lock, never unlock), and a run that asks first is refused with code
`confirmExecute` and a `token` to pass as `confirmed` (where the connection type lets agents
confirm; on Production agents only read and get `agentReadOnly` with SQL to hand to a person);
`db.transaction.commit`, `.rollback`, `.mode`,
`.status` and `.pending` drive transactions, and a refusal with code `openTransactions` names what
to run again with `transactions: "commit"` or `"rollback"`; `db.connection.setSecret { connection, kind, value }`
stores a connection's password in the macOS Keychain (an agent can store one but never read it
back). `db.schema.children { connection, path }`, `db.object.get`, `db.object.ddl` and
`db.schema.refresh` read the structure, `db.sql.complete { path?, offset?, limit? }`,
`db.sql.resolve` and `db.sql.navigate` complete, resolve and open the names of a console,
`editor.code.reformat { path?, wholeFile? }` formats a console or SQL file in place and
`db.sql.format { text, dialect?, connection?, options?, limit? }` formats text an agent gives, and the `db.navigator.*` commands drive the Database
Navigator. `db.console.explain { path?, analyze?, verbose?, costs?, buffers?, settings?, parameters?, variables? }` explains a statement into a Plan tab (refused with `parametersNeeded` like a run), and
`db.plan.state`, `db.plan.select` and `db.plan.view` read its nodes page by page, choose one and show the tree or the text.
`db.key.get { connection, path }` reads a Redis key with its type and TTL,
`db.key.expire { connection, path, seconds? }` sets or removes its expiry and `db.key.open` opens its
value tab. `db.object.open { connection, path, page? }` opens an object tab, `db.object.state`,
`.showPage`, `.showSection`, `.properties`, `.ddlOptions` and `.readData` drive it,
`db.structure.stage { tab?, change }`, `.state`, `.unstage`, `.discard`, `.preview`, `.execute`,
`.toConsole` and `.saveMigration` edit its structure (a refusal names the reason), and
`db.result.value`, `.saveValue`, `.calc`, `.geometry` and `.toggleValuePanel` read values, selections and their shapes,
`db.object.goTo { query, limit?, connection?, kinds? }` finds objects, columns, scripts and statements
by name with the command that opens each. `db.history.list { query?, connection?, sources?, outcome?, period?, limit? }`
searches the query history (statements agents ran are marked `agent`), `db.history.get`,
`.openInConsole`, `.open`, `.clear` and `.settings` read, reuse and manage it; `db.recovery.list`,
`.get`, `.restore`, `.discard` and `.open` review what was kept after a crash. `db.session.list { connection, showIdle?, limit?, textLimit? }`
lists the server's sessions with their blocking chains and locks (queries cut and passwords masked),
`db.session.state` follows it, `db.session.open` opens the Sessions tab, and `db.session.cancelQuery`
and `db.session.terminate { connection, session }` always ask first (code `confirmExecute` with a
`token` to pass as `confirmed`; on Production agents get `agentReadOnly` and hand it to a person).
`db.migration.list` lists the project's migration folders, `db.migration.status { set, connection? }`
tells where each migration stands (never connecting), `db.migration.run { set, connection, direction }`
runs the tool in the Run window after the same questions as a person gets, and `db.migration.open`
opens the Migrations tab.
`db.connection.openTerminal { connection }` starts the connection's client in the Terminal (for
agents only with full access on a connection that is neither read-only nor asks before changes, and
never with a stored password), `db.connection.clientCommand` answers
its command line, and `db.object.generateCode { connection, path, language }` writes a Go, Rust or
TypeScript type from a table. Agents never see a password,
and the `workspace.host.*` and `workspace.tab.*` commands expand, collapse and drive the workspace
(`workspace.host.toggle { id: "database" }`). `get_document_text` reads a console like any file.
