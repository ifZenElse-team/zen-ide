# Zen Workbench: schema and data compare in the Database workspace.

database-compare-title = Compare
database-compare-error-unknown = There is no comparison { $id }.
database-compare-error-not-relational = Only SQL databases can be compared.
database-compare-error-not-connected = { $connection } is not connected, so its structure could not be read.
database-compare-error-no-schema = { $connection } has no schema { $schema } in its structure. Refresh it in the Database Navigator first.
database-compare-error-no-file = The snapshot file { $path } does not exist.
database-compare-error-snapshot = The snapshot file { $path } could not be read: { $error }
database-compare-error-snapshot-version = The snapshot file was written by another version (format { $version }).
database-compare-error-not-finished = The comparison has not finished.
database-compare-error-no-table = Name the table to compare.
database-compare-error-no-key = The table has no primary key in its structure: name the key columns.
database-compare-error-no-columns = The table's columns are not known yet: refresh its structure or name the columns.
database-compare-error-not-read = The schema { $schema } has not been read yet: refresh it first.
database-compare-note-not-dropped = { $object } is only in the target; not dropped.
database-compare-note-not-dropped-part = { $part } of { $object } is only in the target; not dropped.
database-compare-note-cannot-alter = { $object } { $part }: this database cannot change it in place; rebuild it by hand.
database-compare-note-by-hand = { $object } { $part }: change it by hand.
database-compare-note-no-ddl = { $object }: no DDL could be generated (its details were not read).
database-compare-note-rows-not-deleted = { $count ->
    [one] 1 row only in the target is not deleted.
   *[other] { $count } rows only in the target are not deleted.
}
database-compare-note-rows-truncated = Only the first { $kept } differing rows were kept; compare again after running this script.
