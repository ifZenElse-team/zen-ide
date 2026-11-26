# ADR-0182: The SQL plug-in on ABI v1, the dialect a plug-in reads in, and where it is bundled

- **Status:** Proposed
- **Date:** 2026-10-28
- **Extends:** [ADR-0056](0056-plugin-abi-v1.md), [ADR-0063](0063-plugin-colours-and-typing.md),
  [ADR-0135](0135-file-types-by-content-and-xml-in-every-ide.md), [ADR-0036](0036-bundled-plugins.md),
  [ADR-0164](0164-database-engine-zen-db-runtime-and-drivers.md)

## Context

The SQL plug-in (`plugins/sql`) only highlighted (ABI v0). A `.sql` file outside the Database
workspace, or anywhere when database support is off, had no folding, no Structure, no completion and
no syntax problems; inside the workspace, completion and the unknown-name warnings come from the
bound connection's snapshot (`zen_db::assist`), which needs a binding. The requirement: folding,
outline, keyword completion and syntax problems for any `.sql` file without a connection, in the
dialect of the bound connection when there is one, and the plug-in bundled where database support
is on.

SQL is not one language: `#` is a comment in MySQL and an operator in PostgreSQL, a backslash
escapes a quote in MySQL's strings and not in standard ones, `$tag$ … $tag$` is a string only in
PostgreSQL, `[name]` is a name in SQL Server and SQLite and a subscript in PostgreSQL, and a
trailing comma before `FROM` is accepted by ClickHouse alone. A plug-in that reads every file as a
superset reports problems that are not there. The ABI gives a plug-in only the text.

## Decision

1. **The SQL plug-in implements ABI v1** with `zen_fold`, `zen_outline`, `zen_complete`,
   `zen_problems` and the new `zen_dialect`; highlighting stays as it was (every dialect at once).
   It reads the text with its own small lexer and statement splitter (strings, quoted names,
   comments, dollar quotes, parameters, `DELIMITER` lines, `GO` lines, psql `\` lines, `BEGIN …
   END` and `CASE … END` blocks), with no dependency, as every plug-in.
   - Folding: statements, brackets, `BEGIN`/`CASE … END` blocks, block comments, runs of line
     comments, multi-line strings (function bodies), `-- region` … `-- endregion`.
   - Structure: one entry per statement; what `CREATE` makes by its name (a table as `class` with
     its columns as `field`s, a view as `interface`, a function or procedure as `function`, a
     trigger as `method`, an index as `key`, a schema or database as `module`, a sequence as
     `constant`, a type or domain as `type`); other statements by their verb and target
     (`INSERT INTO orders`, `SELECT … FROM shop.orders`, `DROP TABLE users`), a `WITH` query's
     common table expressions inside it as `variable`s.
   - Completion: the dialect's keywords (with phrases such as `ORDER BY`), data types and built-in
     functions (`name($0)`), what usually follows the previous keyword first, statement starters
     first at the start of a statement, types first after a column name in `CREATE`/`ALTER`, after
     `::` and in `CAST(… AS`; keywords in the case typed (ClickHouse's case-sensitive types as they
     are spelled); nothing inside a literal, a comment, a quoted name, after `.` or in a parameter.
   - Problems: unclosed strings, quoted names, comments and dollar-quoted strings; brackets that do
     not match or are not closed by the end of the statement; `CASE` without `END`; a comma before
     `)`, a clause (`FROM`, `WHERE`, `GROUP`, `ORDER`…) or the end of a statement, except in
     ClickHouse. The messages are the plug-in's own, in English, as for every plug-in (ADR-0135).
2. **`zen_dialect`, an optional ABI v1 export** with the signature of the others: its input is the
   UTF-8 name of a dialect, its answer has a zero length. A host that knows it calls it before
   **every** call of any other export of the module, with the file's dialect or an empty name, all
   under the module's lock, so no answer is ever read in the dialect of another file. An IDE that
   does not know it never calls it and the module reads every file in its default; a module without
   it is never told. The names are those of the drivers' languages without `sql:`: `postgres`,
   `mysql`, `mariadb`, `sqlite`, `clickhouse`, and `sqlserver` for later; anything else is "no
   dialect".
3. **The host's side.** `zen_syntax::TextFeatures::in_dialect(dialect)` answers the same plug-in
   bound to a dialect (`None` when it does not tell dialects apart); `WasmHighlighter` shares its
   module between the views, and `zen_plugins::native` does the same for the host's tests.
   `zen_workbench::plugins::plugin_for` asks the database module for the dialect of the connection
   the file is bound to (a console, or a project `.sql` file bound with `db.script.bind`) and uses
   that view for folding, the Structure, completion and problems; the outline and problems caches
   keep the dialect they were made in. Without database support (and in the WebAssembly build of the
   workbench) there is no dialect.
4. **Without a dialect the plug-in reads what most dialects accept** and reports a problem only when
   it finds it under both PostgreSQL's and MySQL's reading; a comma before a clause is then a
   warning, since some engines accept it.
5. **One source of completion in a bound file.** In a console or a bound file the connection's
   completion (`zen_db::assist`) already offers the dialect's keywords with the schema's names; the
   plug-in's items are not added there, so nothing is listed twice. In any other `.sql` file the
   plug-in's items are the list. Problems join: the plug-in's syntax problems and the connection's
   unknown-name warnings both show.
6. **Bundled where database support is on by default**: `sql` joins the bundles of GoGarden,
   RustRonin and WebWave (`xtask/src/release.rs`, `PRODUCT_PLUGINS`); a test ties the list to the
   apps' module defaults, so an IDE that turns the Database module on ships SQL with it. JvmJizo
   keeps offering it from the catalog, as before. The plug-in's version becomes 0.2.0; the catalog records
   ABI 1, so an IDE older than ABI v1 keeps 0.1.0.

## Consequences

- Everything the Database workspace does with SQL keeps working without the plug-in: completion,
  navigation, warnings, splitting and formatting are `zen_db`'s, and the console still offers the
  plug-in when it is missing.
- Highlighting does not take the dialect yet: it goes through the editor's highlighter, which knows
  no binding. `#` in a PostgreSQL file is still coloured as a comment.
- A file's dialect is that of its binding; a file's extension (`.pgsql`, `.mysql`) does not choose
  one yet.
- Folds are kept by the document per version (ADR-0056); a binding changed without an edit refolds
  at the next edit.

## Alternatives considered

- **The dialect in every export's input** (a prefix before the text): changes the input of every
  existing export, so v1 modules of other plug-ins would break; a new ABI version for one parameter.
- **A dialect per file type** (`.pgsql` → PostgreSQL): no way to follow the bound connection, which
  is what is required, and consoles are all `.sql`.
- **Telling the module once and keeping it set**: a module is shared by every file of its type and
  called from several threads; a dialect left from another file would answer for this one.
- **Folding, outline and problems in the workbench from `zen_db::sql`**: works only where database
  support is compiled in and on, and the requirement covers any `.sql` file in every IDE.
