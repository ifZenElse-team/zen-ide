database-copy-error-empty-delimiter = The column and row delimiters cannot be empty.
database-copy-error-delimiter-quote = A delimiter cannot hold the quote character.
database-copy-cut-rows = Copied { $rows } of { $total } rows: a copy holds at most 100 000 rows. Export the result to take every row.
database-copy-cut-size = Copied { $rows } of { $total } rows: a copy holds at most 64 MiB of text. Export the result to take every row.
database-copy-cut-limit = The answer holds { $rows } of { $total } rows; ask with a larger limit for more.
database-copy-cancelled = The copy was cancelled; the clipboard is unchanged.
database-copy-running = Copying { $total } rows…
database-copy-error-unknown-job = There is no copy { $job }.
database-paste-error-empty = There is nothing to paste.
database-paste-error-too-large = A paste holds at most { $most } cells.
database-paste-no-column = There is no column { $column } here.
database-paste-deleted-row = The row is marked to be deleted.
database-paste-error-invalid = { $count ->
    [one] One value does not fit its column; nothing was pasted.
   *[other] { $count } values do not fit their columns; nothing was pasted.
}
