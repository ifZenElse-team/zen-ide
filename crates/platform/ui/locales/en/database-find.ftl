# Zen UI: the Find in Database tab of the Database workspace.

ide-dbsearch-open = Find in Database
ide-dbsearch-text = Text to find
ide-dbsearch-no-connection = No connection
ide-dbsearch-no-schema = Schema
ide-dbsearch-schemas-unknown = Connect to list the schemas
ide-dbsearch-find = Find
ide-dbsearch-stop = Stop
ide-dbsearch-data = Data
ide-dbsearch-names = Names
ide-dbsearch-match-case = Match case
ide-dbsearch-views = Views
ide-dbsearch-columns-of = Columns:
ide-dbsearch-kind-text = Text
ide-dbsearch-kind-number = Numbers
ide-dbsearch-kind-temporal = Dates and times
ide-dbsearch-kind-other = Other
ide-dbsearch-only-columns = Only columns:
ide-dbsearch-only-columns-hint = email, name
ide-dbsearch-rows-per-table = Rows per table:
ide-dbsearch-seconds-per-table = Seconds per table:
ide-dbsearch-not-sql = This connection does not use SQL, so it cannot be searched.
ide-dbsearch-intro = Type a text and press Enter: the tables of the schema are searched on the server one at a time, each up to its row limit and deadline.
ide-dbsearch-starting = Starting…
ide-dbsearch-listing = Listing the tables of { $schema }…
ide-dbsearch-running = Searching { $table }… { $ended } of { $total } tables, found in { $found }
ide-dbsearch-failed = The search failed: { $reason }
ide-dbsearch-cancelled = Stopped after { $ended } of { $total } tables, found in { $found }
ide-dbsearch-nothing = Nothing found in { $total } tables ({ $seconds } s)
ide-dbsearch-done = { $rows ->
    [one] { $rows } row
   *[other] { $rows } rows
} in { $found } of { $total } tables ({ $seconds } s)
ide-dbsearch-table = table
ide-dbsearch-view = view
ide-dbsearch-materialized-view = materialized view
ide-dbsearch-rows = { $rows ->
    [one] { $rows } row
   *[other] { $rows } rows
}
ide-dbsearch-rows-more = { $rows }+ rows
ide-dbsearch-timeout = stopped after { $seconds } s
ide-dbsearch-table-failed = failed: { $reason }
ide-dbsearch-name-matches = its name matches
ide-dbsearch-named-columns = columns named { $columns }
ide-dbsearch-open-rows = Open Rows
ide-dbsearch-open-column = Only Rows with It in { $column }
ide-dbsearch-menu = Find in Database…
