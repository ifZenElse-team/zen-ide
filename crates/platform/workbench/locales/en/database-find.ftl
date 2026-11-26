# Zen Workbench: Find in Database, the search of a schema's tables on the server.

database-find-title = Find in Database
database-find-warning-production = { $kind } connection: the search reads every chosen column of every table on the server.
database-find-warning-large = The tables of { $schema } hold about { $rows } rows: searching them loads the server; each table stops at its limit and deadline.
database-find-warning-views = Views run their queries to be searched, which can be slow.
database-error-search-not-sql = { $name } does not use SQL, so it cannot be searched.
database-error-search-no-text = Enter the text to find.
database-error-search-nothing = Choose to search the data, the names or both.
database-error-search-no-schema = Choose the schema to search: the connection's structure has not been read yet.
database-error-unknown-search = There is no search { $id }.
database-error-no-search = Nothing has been searched yet.
database-error-search-unknown-table = The search did not look in { $table }.
database-error-search-no-columns = None of these columns has the text.
