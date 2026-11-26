# Zen Workbench: the Database workspace and its consoles. English is the source language.

module-database-name = Database
module-database-description = The Database workspace: SQL consoles kept in the IDE's data folder, ready for connections, the Database Navigator and data editors as they come.
database-workspace-title = Database
database-overview-title = Overview
database-error-no-data-folder = The IDE has no data folder to keep consoles in.
database-error-connection-name = “{ $name }” cannot name a connection's folder of consoles: use a plain name without slashes, colons or a leading dot.
database-error-no-connection = There is no connection to open a console on: add one first with New Connection….
database-error-no-connection-agent = There is no connection to open a console on: add one first with `db.dataSource.add`.
database-error-files = The database connection files cannot be read or written: { $error }
database-error-unknown-connection = There is no database connection “{ $id }”.
database-error-not-external-driver = There is no external database driver “{ $id }”.
database-error-unknown-driver = There is no database driver “{ $driver }”.
database-error-bad-url = The connection URI cannot be read: write it as scheme://user@host:port/database.
database-error-no-name = A name is needed.
database-error-color = “{ $color }” is not a colour of the theme; use one of { $colors }.
database-error-unknown-type = There is no connection type “{ $id }”.
database-error-type-exists = A connection type “{ $id }” exists already.
database-error-type-built-in = { $name } is a built-in connection type and cannot be deleted.
database-error-type-used = { $name } is used by { $connections }; give them another type first.
database-error-not-replayed = The connection was lost while the statement ran. The connection is open again, but the statement was not run again: check whether it took effect. { $detail }
database-error-rolled-back = The transaction had failed, so the server rolled it back instead of committing it: none of its changes were saved.
database-error-cancel-unconfirmed = The server did not confirm the cancel within a second; the session was closed and opens again on next use.
database-error-no-server-cancel = This database cannot cancel on the server; the session was closed and opens again on next use.
database-error-connection = The database cannot be reached: { $detail }
database-error-authentication = The server refused the user name or password: { $detail }
database-error-tls = TLS failed: { $detail }
database-error-password-needed = This connection needs a password: enter it, and tick Save password to keep it.
database-error-timeout = The deadline passed.
database-error-no-columns = The statement returned no columns to show.
database-error-cancelled = Cancelled.
database-error-server = { $detail } [{ $code }]
database-error-not-connected = The connection is not connected; connect it first.
database-error-open-transactions = Open transactions in { $contexts }: commit or roll them back, or disconnect with transactions set to commit or rollback.
database-error-invalid = The connection's settings are not usable: { $detail }
database-error-not-repeatable = More rows need running the statement again, and it may change data; run it again in a console to read on.
database-error-unsupported = Not supported here: { $detail }
database-error-cancel-what = Name an operation or a connection to cancel.
database-error-unknown-operation = There is no database operation { $id }; the last 200 are kept.
database-notice-idle-closed = { $name } was idle and its connection closed; it opens again on next use.
database-notice-secret-failed = A connection's secret could not be stored or deleted: { $detail }
database-notice-slept = { $count ->
    [one] The computer slept, so its database connection closed; it opens again on next use.
   *[other] The computer slept, so { $count } database connections closed; they open again on next use.
}
database-notice-transaction-ended = An idle transaction of { $name } ({ $context }) was rolled back.
database-error-secret-empty = A secret cannot be empty.
database-error-secret-id = “{ $connection }” cannot name a connection: use its id, without slashes.
database-error-secret-denied = Access to the Keychain item was denied. Allow it in the Keychain prompt and try again.
database-error-secret-timeout = The Keychain did not answer in time, so the saved password could not be read. If macOS shows a Keychain prompt (unlock the screen to see it), allow access there, then connect again.
database-error-secret-unsupported = This system has no secret store the IDE can use; secrets are kept only in the macOS Keychain for now.
database-error-secret-failed = The Keychain could not keep the secret: { $reason }
database-error-secret-no-store = This window has no secret store.
database-cause-dns = Likely cause: the host name is not known; check its spelling and your network or VPN.
database-cause-refused = Likely cause: nothing listens on that host and port; check that the server runs and the port is right.
database-cause-tls = Likely cause: TLS; check the SSL mode and the certificates, or whether the server has TLS at all.
database-cause-authentication = Likely cause: the user name or password; check them and the server's access rules.
database-cause-timeout = Likely cause: the server did not answer in time; check the host, a firewall or an SSH tunnel.
database-error-no-console = No console or .sql file is active; open one or name it with `path`.
database-error-no-console-connection = The console has no connection: choose one in the connection list on the console's toolbar.
database-error-no-console-connection-agent = The console has no connection: choose one with `db.console.chooseConnection`.
database-error-no-statement = There is no statement to run here.
database-error-read-console = Cannot read { $path }: { $error }
database-error-unknown-result = There is no result { $id }; closed results are gone.
database-error-unknown-query = There is no query { $id }; the last 100 are kept.
database-error-confirm-fetch-all = { $rows } rows are read so far. Fetching every row can take long and much memory; confirm to go on.
database-result-title = Result { $number }
database-console-hint-time = { $ms } ms
database-console-hint-rows = { $time } · { $rows ->
    [one] 1 row
   *[other] { $rows } rows
}
database-console-hint-updated = { $time } · { $rows ->
    [one] 1 row updated
   *[other] { $rows } rows updated
}
database-console-hint-failed = failed
database-folder-tables = Tables
database-folder-foreign-tables = Foreign Tables
database-folder-views = Views
database-folder-materialized-views = Materialized Views
database-folder-functions = Functions
database-folder-procedures = Procedures
database-folder-sequences = Sequences
database-folder-types = Types
database-folder-events = Events
database-folder-packages = Packages
database-folder-columns = Columns
database-folder-constraints = Constraints
database-folder-foreign-keys = Foreign Keys
database-folder-indexes = Indexes
database-folder-triggers = Triggers
database-folder-references = References
database-folder-policies = Policies
database-folder-partitions = Partitions
database-folder-dictionaries = Dictionaries
database-navigator-loading = Loading…
database-navigator-not-read = Not read yet: connect to read it.
database-navigator-nothing-selected = Nothing is selected in the Database Navigator; name a connection or a folder.
database-navigator-other-database = { $database } is another database of the server; it is read through a connection of its own.
database-error-unknown-object = There is no object { $path } in connection { $connection }, as far as its structure was read.
database-error-not-key-value = Connection { $connection } is not a key-value database; its keys cannot be read.
database-error-no-object-tab = No table or other object is open: open one from the Database Navigator.
database-error-no-object-tab-agent = No object tab is active; open one with `db.object.open` or name it with `tab`.
database-error-object-no-data = { $name } has no rows to show.
database-error-object-page = This object has no { $page } page.
database-error-object-section = This object has no section “{ $section }”; its sections are { $sections }.
database-error-no-cell = There is no cell at row { $row }, column { $column }: the result has { $rows } rows and { $columns } columns.
database-error-save-folder = { $path } is a folder; name a file.
database-error-save-exists = { $path } exists; pass overwrite to replace it.
database-object-section-columns = Columns
database-object-section-constraints = Constraints
database-object-section-foreign-keys = Foreign Keys
database-object-section-indexes = Indexes
database-object-section-triggers = Triggers
database-object-section-references = References
database-object-section-policies = Policies
database-object-section-partitions = Partitions
database-object-section-statistics = Statistics
database-object-section-permissions = Permissions
database-object-section-source = Source
database-object-section-settings = Settings
database-object-section-objects = Objects
database-object-yes = yes
database-object-identity = identity ({ $how })
database-object-generated = generated: { $expression }
database-object-column-name = Name
database-object-column-type = Type
database-object-column-not-null = Not Null
database-object-column-default = Default
database-object-column-collation = Collation
database-object-column-comment = Comment
database-object-column-kind = Kind
database-object-column-columns = Columns
database-object-column-definition = Definition
database-object-column-references = References
database-object-column-on-update = On Update
database-object-column-on-delete = On Delete
database-object-column-unique = Unique
database-object-column-method = Method
database-object-column-size = Size
database-object-column-function = Function
database-object-column-enabled = Enabled
database-object-column-schema = Schema
database-object-column-table = Table
database-object-column-constraint = Constraint
database-object-column-command = Command
database-object-column-permissive = Permissive
database-object-column-roles = Roles
database-object-column-using = Using
database-object-column-check = With Check
database-object-column-partition = Partition
database-object-column-bound = Bound
database-object-column-property = Property
database-object-column-value = Value
database-object-column-grantee = Grantee
database-object-column-privileges = Privileges
database-object-column-grantor = Granted By
database-object-column-rows = Rows
database-object-primary-key = Primary key
database-object-unique-key = Unique key
database-object-check = Check
database-object-exclusion = Exclusion
database-object-foreign-key = Foreign key
database-object-partitioned-by = Partitioned by { $key }
database-object-partition-of = A partition of { $parent } { $bound }
database-object-row-estimate = Rows (estimate)
database-object-total-size = Total size
database-object-index-count = Indexes
database-object-index-size = Index size
database-object-column-count = Columns
database-object-row-security = Row security
database-object-data-type = Data type
database-object-start = Start
database-object-increment = Increment
database-object-minimum = Minimum
database-object-maximum = Maximum
database-object-cache = Cache
database-object-cycle = Cycle
database-object-owned-by = Owned by
database-object-labels = Labels
database-object-base-type = Base type
database-object-checks = Checks
database-object-owner = Owner
database-object-tablespace = Tablespace
database-object-default-tablespace = (default)
database-object-arguments = Arguments
database-object-returns = Returns
database-object-language = Language

## Filtering and sorting data views
database-error-no-active-result = No result is shown: open a table's data or run a query in a console.
database-error-filter-semicolon = A filter is one condition: remove the ";".
database-error-filter-parentheses = The parentheses of the filter do not match.
database-error-filter-unterminated = A quote or a comment of the filter is not closed.
database-error-filter-not-query = Only a query that reads rows can be filtered; find in the rows loaded instead.
database-error-filter-pending-changes = Save or cancel the data changes before filtering or sorting again.
database-error-filter-no-row = Choose the cell whose value to filter by.
database-error-filter-empty = There is no condition to save.
database-error-columns-pending = The result's columns have not come yet: wait until its rows are read.
database-error-reading = The rows are being read again: wait until they come, then change them.
database-question-staged = Results with data changes that are not saved: { $count }. Save or cancel the changes first.
database-error-no-column = There is no column { $column }.
database-error-go-to-row = There is no row { $row }: { $rows } rows are read; Fetch Next or Fetch All reads more.
database-error-cannot-sort = These rows cannot be read again and not all of them are loaded: fetch all rows to sort them.

## Editing data
database-read-only-connection = Read-only: the connection is read-only.
database-read-only-driver = Read-only: this database's results cannot be edited.
database-read-only-no-table = Read-only: the rows do not come from a table.
database-read-only-several-tables = Read-only: the rows join several tables.
database-read-only-unknown-table = Read-only until the table's structure is read.
database-read-only-no-key = Read-only: no key identifies the rows. Choose a key, or use all columns.
database-read-only-no-row-identity = Read-only: this database has no row identity, so a change cannot find one row.
database-error-saving = The changes are being saved.
database-error-read-only-column = The column { $column } cannot be changed: it is generated or computed.
database-error-not-null = The column { $column } cannot be NULL.
database-error-input-integer = Not a whole number.
database-error-input-number = Not a number.
database-error-input-boolean = Not true or false.
database-error-input-json = Not valid JSON.
database-error-input-uuid = Not a UUID.
database-error-input-bytes = Not bytes: write them as \x and hexadecimal digits.
database-error-input-object-id = Not an ObjectId: write 24 hexadecimal digits.
database-error-save-conflict = Row { $row } was changed or deleted by someone else: no row was changed. Nothing was saved.
database-error-save-several = Saving row { $row } would change several rows: they are equal in the columns that identify a row. Nothing was saved.
database-error-save-row = Row { $row }: { $error } Nothing was saved.
database-error-save-kept = Row { $row }: { $error } The changes before it stay saved, as this database saves them without a transaction; refresh the result to see them.
database-error-save-gone = the document was changed or deleted by someone else.
database-error-confirm-data-change = The connection type asks to confirm data changes: { $count } statements will run.
database-error-invalid-key = A key is made of the table's columns in the result: { $columns }.
database-question-pending-one = { $name } ({ $context }) has { $count ->
    [one] one statement
   *[other] { $count } statements
} that may have changed data and are not committed.
database-error-ssh = The SSH tunnel did not open: { $detail }
database-error-unknown-detection = This detected database is no longer found in the project.
database-error-unknown-detection-agent = There is no detected database { $id } in this project; list them with `db.detect.list`.
database-error-import-no-path = Name the folder or file to import from.
database-error-import-none = No { $file } was found in { $path }.
database-error-import-read = { $file } could not be read: { $error }
database-error-import-too-large = the file is larger than 8 MB
database-import-unsupported = The IDE has no driver for { $product } yet.
database-error-sqlite-missing = The sqlite3 program is needed for SQLite and was not found ({ $detail }). macOS has it; elsewhere install SQLite from sqlite.org or your package manager, or set the program's path on the connection's Advanced page.
database-error-file-missing = { $detail }: the database file does not exist. Choose an existing file with Open… or make a new one with Create… on the connection's Main page.
database-file-no-path = Name the database file.
database-file-not-absolute = { $path } is relative and no project is open: give an absolute path.
database-file-exists = { $path } exists already and was not replaced; open it with Open… instead.
database-file-failed = { $path } could not be created: { $error }
database-file-missing = { $path } does not exist.
database-error-export-not-table = { $path } is not a table or a view, so it has no rows to export.
database-error-export-changes = Export runs its statement again, and this one may change data. Export the rows already loaded, or run the statement in a console.
database-error-export-nothing = Choose at least one table to export.
database-error-export-delimiter = The delimiter must not be empty, a line break or the quote character.
database-error-export-one-path = A file path names one file; export several tables into a folder with a file name pattern.
database-error-export-binary = Parquet files are binary: export them to a file, not the clipboard.
database-error-export-cells-binary = Selected cells are exported as text: choose a format other than Parquet, or export every row.
database-error-export-no-folder = The folder { $path } does not exist.
database-error-export-exists = { $count ->
    [one] { $path } exists already.
   *[other] { $count } files exist already, { $path } among them.
}
database-error-export-is-folder = A folder has the name of a file to export.
database-error-unknown-export = There is no export { $id }.
database-export-title = Export
database-export-cancelled = The export was cancelled; no file was left half written.
database-export-failed = The export failed:
    { $reasons }
database-export-copied = { $rows ->
    [one] One row was copied to the clipboard.
   *[other] { $rows } rows were copied to the clipboard.
}
database-export-done-file = { $rows ->
    [one] One row was exported to { $path }.
   *[other] { $rows } rows were exported to { $path }.
}
database-export-done-files = { $rows } rows were exported to { $count } files in { $folder }.
database-export-open = Open
database-export-reveal = Reveal in Finder

## SQL assistance in consoles
database-sql-kind-table = Table
database-sql-kind-foreign-table = Foreign table
database-sql-kind-view = View
database-sql-kind-materialized-view = Materialized view
database-sql-kind-function = Function
database-sql-kind-procedure = Procedure
database-sql-kind-sequence = Sequence
database-sql-kind-type = Type
database-sql-kind-event = Event
database-sql-kind-package = Package
database-sql-kind-column = Column
database-sql-not-null = not null
database-sql-primary-key = primary key
database-sql-not-read = Its columns have not been read yet: they are read when the connection is connected.
database-sql-more-columns = { $count ->
    [one] …and { $count } more column
   *[other] …and { $count } more columns
}
database-sql-unknown-table = Unknown table { $name }
database-sql-unknown-column = Unknown column { $name }
database-sql-unknown-column-of = Unknown column { $name } in { $table }
database-error-read-only-statement = The connection is read-only: { $verb } was not run because it may change data or structure.
database-error-refused = Not run: { $detail }
database-error-read-only-run = { $name } is read-only: { $statements } would change data or structure, so nothing was run.
database-error-confirm-execute = { $name } is a { $kind } connection: { $count ->
    [one] a statement changes
   *[other] { $count } statements change
} data or structure. Confirm to run.
database-error-dangerous = Confirm before running on { $name }: { $risks }.
database-confirm-commit = { $commit ->
    [auto] Auto-commit is on: each change is committed at once.
   *[manual] Manual commit: the changes stay in the open transaction until you commit or roll back.
}
database-type-kept-manual = { $name } took its new connection type; { $count ->
    [one] a session with uncommitted changes keeps
   *[other] { $count } sessions with uncommitted changes keep
} manual commit until you commit or roll back.
database-error-all-skipped = Every statement that asked was skipped and nothing else is left to run.
database-error-agent-not-allowed = Agents may not run anything on { $name } ({ $kind } connection type).
database-error-agent-read-only = Agents may only read on { $name } ({ $kind }): { $statements } was not run. A person can review and run it: open it in a console with db.console.new.
database-error-agent-outside = Agents may run SQL only on the database of { $name }: { $statements } reaches files, extensions or other servers and was not run. A person can review and run it: open it in a console with db.console.new.
database-error-agent-change = Agents may only read on { $name } ({ $kind }): saving, committing and changing keys are left to a person.
database-error-agent-loosen = Agents may not make { $name } less safe: read-only, its connection type and its confirmations are the person's to loosen.
database-error-agent-loosen-type = Agents may not make the connection type { $name } less safe.
database-error-agent-program = Agents may not choose the program a driver runs (the setting { $setting }): that is the person's choice.
database-risk-delete = DELETE without WHERE removes every row of { $object }
database-risk-delete-unknown = DELETE without WHERE removes every row
database-risk-update = UPDATE without WHERE changes every row of { $object }
database-risk-update-unknown = UPDATE without WHERE changes every row
database-risk-drop = DROP removes { $object } with its data
database-risk-drop-unknown = DROP removes an object with its data
database-risk-truncate = TRUNCATE removes every row of { $object }
database-risk-truncate-unknown = TRUNCATE removes every row
database-risk-flush = FLUSH removes every key of the database
database-error-script-not-bound = { $path } is bound to no connection: choose one above the editor.
database-error-script-not-bound-agent = { $path } is bound to no connection: choose one with `db.script.bind`.
database-error-code-not-bound = SQL in { $path } is read against no connection: choose one for this file or for the whole project.
database-error-code-not-bound-agent = SQL in { $path } is read against no connection: choose one with `db.injection.bind`, or set the project's default with `db.injection.setDefault`.
database-error-not-code = SQL in string literals is looked for in Go, Rust, Python, JavaScript, TypeScript, Java and Kotlin files; open one or name it with `path`.
database-error-no-fragment = No string literal at the caret holds SQL: put the caret inside the query's quotes.
database-error-choose-connection = Choose the connection of the SQL in this file: press ⌥⏎ inside the query's string and choose Choose SQL Connection….
database-error-choose-connection-agent = Choose the connection of the SQL in this file with `db.injection.bind`, or the project's with `db.injection.setDefault`.
database-injection-run = Run SQL
database-injection-choose-connection = Choose SQL Connection…
database-injection-change-connection = SQL Connection: { $name }…
database-error-console-unbind = A console always runs on a connection: choose another one instead.
database-error-script-folder = Scripts go in a folder of the project that is not hidden: { $path }
database-error-script-exists = { $path } already exists.
database-error-parameters-needed = { $count ->
    [one] The statement needs a value: give it in the parameters dialog or as `parameters`.
   *[other] The statements need { $count } values: give them in the parameters dialog or as `parameters`.
}
database-error-invalid-parameter = The value of { $name } is wrong: { $error }
database-error-variable-name = Not a variable name: { $name }
database-error-variables-not-saved = The variables could not be saved: { $error }
database-error-script-command = The script command is wrong: { $error }
database-error-include = { $file } could not be included: { $error }
database-include-missing = no such file next to the script or in the project
database-include-cycle = it includes itself
database-include-too-deep = includes nest deeper than { $most }
database-parameters-inlined = Written into the statement as quoted literals, as this driver has no bind parameters: { $values }
database-result-on-connection = { $connection } · { $title }
database-error-several-none = Choose at least one connection to run the script on.
database-error-several-too-many = A script runs on at most { $most } connections at once; { $count } were chosen.
database-error-several-running = This console is already running on several connections; cancel that run first.
database-error-several-no-run = This console has not run on several connections.
database-error-several-refused = Nothing ran: { $count ->
    [one] a connection refuses
   *[other] { $count } connections refuse
} the script: { $names }. { $reason }
database-error-several-confirm = Confirm the run on { $count ->
    [one] one connection
   *[other] { $count } connections
}: { $asking ->
    [one] one asks
   *[other] { $asking } ask
} before changes ({ $names }).
database-error-unknown-profile = There is no database profile “{ $profile }”.
database-notice-profile-detected = The server of { $name } is { $profile }. Its structure is read the way { $profile } needs; the connection's settings stay as they are.
database-notice-profile-use = Use { $profile } Profile
database-param-error-number = not a number: { $text }
database-param-error-boolean = not a boolean: { $text }
database-param-error-value = not a parameter value: give text, a number, a boolean, null or {"{"} "type", "value" {"}"}
database-param-error-include = @include needs a file
database-param-error-set = @set needs name = value
database-param-error-name = not a variable name: { $name }
database-param-error-no-value = no value
database-error-agent-closed = { $name } is closed to agents ({ $kind } connection type): agents see nothing of it, neither its data nor its objects and structure.

## Removing a connection

database-remove-question = Remove the connection “{ $name }”? { $tabs ->
    [0] No tab closes.
    [one] 1 tab closes.
   *[other] { $tabs } tabs close.
}{ $consoles ->
    [0] {""}
    [one] {" "}1 console stays, without a connection.
   *[other] {" "}{ $consoles } consoles stay, without a connection.
}
database-usages-title = Usages of { $name }
database-usages-rename-title = Rename { $name } to { $to }
database-usages-target = { $kind } { $name }
database-usages-rename-target = { $target }, to be renamed to { $to }
database-usages-scope = the project and { $connection }
database-usages-consoles = Consoles
database-usages-kind-trigger = Trigger
database-usages-kind-foreign-key = Foreign key
database-usages-no-structure = The structure of this connection is not read yet: connect to it or refresh it in the Database Navigator first.
database-usages-unknown = { $name } is not in the structure of this connection.
database-usages-unsupported = Find Usages works on tables, views, routines and columns.
database-usages-no-new-name = Type the new name.
database-folder-aggregates = Aggregate Functions
database-folder-extensions = Extensions
database-folder-event-triggers = Event Triggers
database-folder-tablespaces = Tablespaces
database-folder-roles = Roles
database-object-no = no
database-object-section-dependencies = Dependencies
database-object-section-rules = Rules
database-object-column-direction = Direction
database-object-column-instead = Instead
database-object-uses = uses
database-object-used-by = used by
database-object-primary = Primary key
database-object-version = Version
database-object-relocatable = Relocatable
database-object-event = Event
database-object-tags = Command tags
database-object-location = Location
database-object-options = Options
database-object-login = Can log in
database-object-superuser = Superuser
database-object-create-database = Can create databases
database-object-create-role = Can create roles
database-object-replication = Replication
database-object-bypass-row-security = Bypasses row security
database-object-connection-limit = Connection limit
database-object-no-limit = no limit
database-object-valid-until = Password valid until
database-object-member-of = Member of
