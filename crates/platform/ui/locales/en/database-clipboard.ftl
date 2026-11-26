ide-database-grid-copy-names = Copy Column Names
ide-database-grid-copy-row-json = Copy Row as JSON
ide-database-grid-copy-as = Copy As
ide-database-grid-copy-advanced = Advanced Copy…
ide-database-grid-paste = Paste
ide-database-grid-paste-rows = Paste as New Rows
ide-database-grid-paste-advanced = Advanced Paste…
ide-database-copy-advanced-title = Advanced Copy
ide-database-copy-header = Column names first
ide-database-copy-row-numbers = Row numbers
ide-database-copy-column-delimiter = Column delimiter:
ide-database-copy-row-delimiter = Row delimiter:
ide-database-copy-quote = Quote character:
ide-database-copy-quoting = Quote values:
ide-database-copy-quoting-needed = When needed
ide-database-copy-quoting-always = Always
ide-database-copy-quoting-never = Never
ide-database-copy-null = NULL as:
ide-database-copy-values = Values:
ide-database-copy-values-display = As shown
ide-database-copy-values-sql = SQL literals
ide-database-copy-values-json = JSON
ide-database-copy-button = Copy
ide-database-copy-running = Copying rows in the background…
ide-database-copy-cancel = Cancel Copy
ide-database-copy-done = Copied { $rows ->
    [one] { $rows } row
   *[other] { $rows } rows
}.
ide-database-paste-title = Advanced Paste
ide-database-paste-tab = Tab
ide-database-paste-summary = Rows: { $rows }, columns: { $columns }, delimiter: { $delimiter }
ide-database-paste-delimiter = Delimiter:
ide-database-paste-detect = Detect
ide-database-paste-header = The first line names the columns
ide-database-paste-mode = Paste:
ide-database-paste-overwrite = Over the cells from the selected one
ide-database-paste-insert = As new rows
ide-database-paste-null = NULL from:
ide-database-paste-problems = { $count ->
    [one] One value does not fit its column and will be left out:
   *[other] { $count } values do not fit their columns and will be left out:
}
ide-database-paste-problem = Line { $line }, { $column }: { $message }
ide-database-paste-ready = Cells to change: { $cells }, new rows: { $rows }.
ide-database-paste-valid = Paste Valid Cells
ide-database-paste-button = Paste
ide-database-paste-done = Pasted cells: { $cells }, new rows: { $rows }. Save to write them to the database.
