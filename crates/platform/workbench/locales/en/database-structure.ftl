database-alter-reason-driver = The { $subject } driver cannot change structure here.
database-alter-reason-schemas = SQLite has no schemas to create, rename or drop: each is an attached database file.
database-alter-reason-rename-schema = MySQL and MariaDB cannot rename a database.
database-alter-reason-sequences = This server has no sequences.
database-alter-reason-routines = SQLite has no functions or procedures written in SQL.
database-alter-reason-comments = This server keeps no comment on { $subject }.
database-alter-reason-rename = This server cannot rename { $subject }.
database-alter-reason-cascade = This server cannot drop { $subject } with what depends on it (CASCADE); drop those objects first.
database-alter-reason-identity = SQLite numbers only a single INTEGER primary key column: { $subject }.
database-alter-reason-constraint-kind = Exclusion constraints are not created here; write them in a console.
database-alter-reason-generated = { $subject } is a generated column; change it in a console.
database-alter-reason-restate = MySQL changes a column by writing it out whole, and this one has { $subject }, which would be lost; change it in a console.
database-alter-reason-rebuild-loses = SQLite would rebuild the table, and its definition has { $subject }, which the rebuild would lose; change it in a console.
database-alter-reason-unknown-schema = There is no schema { $subject }.
database-alter-reason-unknown-object = There is no object { $subject }, or its details are not read yet.
database-alter-reason-unknown-column = There is no column { $subject }.
database-alter-reason-unknown-constraint = There is no constraint { $subject }.
database-alter-reason-unknown-index = There is no index { $subject }.
database-alter-reason-exists = { $subject } already exists.
database-alter-reason-missing = Fill in: { $subject }.
database-alter-reason-not-one-statement = The { $subject } must be one statement or expression.
database-alter-reason-not-empty = Database { $subject } holds objects: drop it with Cascade to drop them too.
database-alter-reason-action = Unknown foreign key action: { $subject }. Use NO ACTION, RESTRICT, CASCADE, SET NULL or SET DEFAULT.
database-alter-reason-run-first = SQLite renamed a column of { $subject } earlier in this script and would now rebuild the table: execute the staged changes first, then make this one.
database-alter-reason-nothing = Nothing changes in { $subject }.
database-alter-note-rebuild = SQLite cannot alter { $table } in place: it is created anew, its rows are copied, the old table is dropped and the new one renamed, and its indexes and triggers are created again.
database-alter-note-implicit-commit = MySQL and MariaDB commit each structure statement on its own: if one fails, the statements before it stay done.
database-alter-note-manual-commit = The connection is in manual commit or has an open transaction: the changes wait in it for Commit.
database-alter-kind-table = table
database-alter-kind-foreign-table = foreign table
database-alter-kind-view = view
database-alter-kind-materialized-view = materialized view
database-alter-kind-function = function
database-alter-kind-procedure = procedure
database-alter-kind-sequence = sequence
database-alter-kind-type = type
database-alter-kind-event = event
database-alter-kind-package = package
database-alter-with-cascade = {" "}with cascade
database-alter-create-schema = Create schema { $name }
database-alter-rename-schema = Rename schema { $name } to { $to }
database-alter-drop-schema = Drop schema { $name }{ $cascade }
database-alter-comment-schema = Change the comment of schema { $name }
database-alter-create-table = Create table { $name } with { $count ->
    [one] { $count } column
   *[other] { $count } columns
}
database-alter-create-view = Create view { $name }
database-alter-create-sequence = Create sequence { $name }
database-alter-create-routine = Create routine { $name }
database-alter-rename = Rename { $kind } { $name } to { $to }
database-alter-drop = Drop { $kind } { $name }{ $cascade }
database-alter-comment = Change the comment of { $kind } { $name }
database-alter-add-column = Add column { $name } { $type } to { $table }
database-alter-alter-column = Change column { $name } of { $table }
database-alter-drop-column = Drop column { $name } of { $table }{ $cascade }
database-alter-add-constraint = Add constraint { $name } ({ $columns }) to { $table }
database-alter-drop-constraint = Drop constraint { $name } of { $table }{ $cascade }
database-alter-create-index = Create index on { $table } ({ $columns })
database-alter-drop-index = Drop index { $name } of { $table }
database-alter-error-running = The structure script of this tab is running; wait for it to end.
database-alter-error-read-only = { $name } is read-only: its structure cannot be changed.
database-alter-error-loading = The details of { $name } are being read; try again in a moment.
database-alter-error-no-change = There is no staged change { $index }: { $count } are staged.
database-alter-error-nothing = No structure changes are staged.
database-alter-error-folder = { $folder } is not a folder inside the project.
database-alter-question-close = { $name } has structure changes that were not run. Discard them?
database-alter-error-skip = The script of structure changes runs whole, as the preview shows it: discard the changes you do not want instead of leaving statements out.
database-alter-error-rebuild-transaction = SQLite rebuilds the table, and inside a transaction it cannot switch foreign keys off, so dropping the old table would change the rows that refer to it. Commit or roll back first, and switch to auto-commit.
database-alter-error-foreign-keys = Whether foreign keys are on could not be read, so the rebuild did not run; nothing changed.
database-alter-note-foreign-keys = Foreign keys are switched off for the rebuild. Execute switches them on again when they were on; the script opened in a console or saved as a migration ends with the statement that does it, as a comment: run it when your database uses foreign keys.
database-loss-object = This deletes { $kind } { $name }.
database-loss-rows = { $count ->
    [one] about { $count } row
   *[other] about { $count } rows
}
database-loss-data = Its data is lost: { $amount }.
database-loss-data-unknown = Every row it holds is lost.
database-loss-parts = Its columns ({ $columns }), indexes ({ $indexes }) and triggers ({ $triggers }) go with it.
database-loss-references = { $count ->
    [one] A foreign key of another table points at it.
   *[other] { $count } foreign keys of other tables point at it.
}
database-loss-view = The data stays in the tables it reads.
database-loss-cascade = With Cascade, every object that depends on it is dropped too.
database-loss-schema = { $count ->
    [one] This deletes schema { $name } with its { $count } object and its data.
   *[other] This deletes schema { $name } with its { $count } objects and their data.
}
database-loss-column = This deletes column { $name } of { $table } with its value in every row.
database-loss-index = This deletes index { $name } of { $table }; the data stays.
database-loss-constraint = This deletes constraint { $name } of { $table }; the data stays but is no longer checked by it.
database-loss-truncate = This deletes every row of table { $name } ({ $amount }); the empty table stays.
database-loss-truncate-unknown = This deletes every row of table { $name }; the empty table stays.
database-loss-truncate-cascade = With Cascade, every row of the tables whose foreign keys point at it is deleted too.
database-error-not-droppable = { $path } cannot be deleted from the navigator.
database-error-not-renamable = { $path } cannot be renamed from the navigator.
database-error-not-truncatable = Only a table can be truncated.
database-error-not-creatable = Nothing of that kind can be created at { $path }.
database-error-rename-name = Type a new name.
database-connection-copy-name = { $name } (copy)
