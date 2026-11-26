# Zen Workbench: Data transfer between databases in the Database workspace.

database-error-transfer-unsupported = { $name } does not take SQL, so tables cannot be copied from or into it.
database-error-transfer-no-table = { $path } is not a table of the source's structure; refresh it and try again.
database-error-transfer-schema = { $path } does not name a schema: give [database, schema].
database-error-transfer-no-tables = Choose at least one table to copy.
database-error-transfer-no-columns = No column goes into { $table }: map at least one.
database-error-transfer-mapping = The mapping of column { $column } is not valid: every source column once, each into its own target column.
database-error-transfer-loading = The columns of the tables are being read; try again in a moment.
database-error-unknown-transfer = There is no transfer { $id }.
database-transfer-title = Data Transfer
database-transfer-done = { $rows ->
    [one] One row was
   *[other] { $rows } rows were
} copied into { $tables ->
    [one] one table
   *[other] { $tables } tables
}.
database-transfer-done-skipped = { $rows ->
    [one] One row was
   *[other] { $rows } rows were
} copied into { $tables ->
    [one] one table
   *[other] { $tables } tables
}; skipped: { $skipped }.
database-transfer-failed = Copying { $table } failed: { $reason }. Rows committed before it stopped: { $committed }.
database-transfer-cancelled = The transfer was cancelled after { $done } of { $tables } tables. Rows committed before it stopped: { $committed }.
database-error-transfer-same-table = { $table } would be copied into itself: choose another target schema or table.
database-error-transfer-too-many = A transfer takes at most { $most } tables; choose fewer.
