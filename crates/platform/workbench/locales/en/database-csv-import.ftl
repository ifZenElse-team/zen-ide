# Zen Workbench: Import from CSV into a table of the Database workspace.

database-error-import-delimiter = The delimiter, the quote and the escape character must differ and none may be a line break.
database-error-import-no-file = { $path } is not a file.
database-error-import-read = { $path } could not be read: { $error }
database-error-import-not-table = { $path } does not name a table: give [database, schema, "tables", name].
database-error-import-unsupported = This connection's driver does not take SQL, so it cannot import CSV files.
database-error-import-table-exists = { $table } exists already; import into it instead of creating it.
database-error-import-no-columns = No column of the file goes into { $table }: map at least one.
database-error-import-mapping = The mapping of column { $column } is not valid: every column once, from a field the file has.
database-error-unknown-import = There is no import { $id }.
database-import-title = Import from CSV
database-import-done = { $rows ->
    [one] One row was imported into { $table }.
   *[other] { $rows } rows were imported into { $table }.
}
database-import-done-skipped = { $rows } rows were imported into { $table }; { $skipped } were skipped:
    { $errors }
database-import-failed = The import into { $table } failed: { $reason }. Rows committed before it stopped: { $committed }.
database-import-cancelled = The import into { $table } was cancelled. Rows committed before it stopped: { $committed }.
database-import-row = Row { $row } (line { $line }): { $reason }
database-import-row-columns = it has another number of fields than the header
database-import-row-number = { $column } is not a number
database-import-row-boolean = { $column } is not true or false
database-import-row-date = { $column } does not match the date and time format
database-error-import-sql-type = The type “{ $sql_type }” of { $column } is not a column type: use letters, digits, spaces, _ . , and balanced parentheses.
