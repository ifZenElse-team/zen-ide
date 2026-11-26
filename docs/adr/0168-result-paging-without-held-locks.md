# ADR-0168: Result paging that never holds a lock outside the person's own transaction

- **Status:** Proposed
- **Date:** 2026-10-26
- **Amends:** [ADR-0164](0164-database-engine-zen-db-runtime-and-drivers.md), decision 7 (the result
  store), for drivers whose results are read through a server-side portal

## Context

ADR-0164, decision 7, keeps a result as pages read from the **open** stream or cursor as the view
needs them, so that the next page never runs the query again. For PostgreSQL an open cursor is a
portal, and a portal lives inside a transaction: a portal kept open between pages in auto-commit
holds the transaction open and, with it, the locks the query took (`AccessShareLock` on every table
it reads), for as long as the person looks at the grid. That blocks `ALTER TABLE`, `VACUUM FULL`
and migrations of everyone else, and keeps old row versions alive, which is the failure our
research warns against. Reading every row at
once instead (the PostgreSQL protocol's default) costs memory and time for results nobody scrolls
through.

## Decision

1. **Outside the person's own transaction** (auto-commit): a query that only reads (`SELECT`,
   `VALUES`, `TABLE`, `WITH` without writes; `zen_db::sql::pageable`) is read through a portal in a
   transaction of the IDE's own: the first page (200 rows, a setting) shows at once, reading goes
   on in the background up to a **row cap** (10 000 rows, a setting) or the memory budget, and then
   the portal is closed and that transaction committed. No lock outlives the reading.
2. **Inside the person's own transaction** (manual commit, or after their `BEGIN`), the portal of a
   run's last result **stays open** and pages are read as the grid scrolls: the locks are the
   person's own already. The next run in that console, Cancel, closing the result, a count or a
   disconnect closes it. Connections that are not Production may opt in to the same outside a
   transaction (`keepOpen`).
3. **Past the cap**, Fetch Next and Fetch All read the statement **again**, skipping the rows kept
   (without decoding them), and append to the same result; the view keeps its scroll, selection and
   columns. Fetch All asks first. The count ("200+" → "200 of 12 345") is a `count(*)` around the
   query, on request.
4. **Never paged:** DML, `RETURNING`, writable CTEs, `SELECT … INTO`, `FOR UPDATE`/`SHARE`, and
   anything behind a transaction pooler: they run to their end in one go, and rows past the cap are
   read and dropped.
5. **Memory:** every result of a window shares the budget of ADR-0164 (256 MiB, a setting); a
   result past it stops reading and says so. Rows are kept as typed columns (integers, floats and
   booleans unboxed, texts in one buffer per column, other values as values), so 100 000 rows of an
   integer and an `md5` text take about 6 MB.

Drivers without server-side paging read their rows at once up to the cap through the same
interface (`DriverSession::stream`, `RowSink`).

## Consequences

- A result read again past the cap may differ from the rows before it if the data changed in
  between; the bottom bar says the rows were read in parts ("stopped at the row cap").
- Results that wait with a portal open count as finished for the person (`db.query.state` says
  done) while the console's session stays busy until the portal closes.
- `zen_db::results` holds the store, the budget and the reader; the PostgreSQL driver uses
  tokio-postgres' portals (`Transaction::bind`, `query_portal_raw`).

## Alternatives considered

- **Keep the portal open in auto-commit** (ADR-0164 as written). Holds locks while the person reads;
  rejected for the reason above.
- **`LIMIT`/`OFFSET` rewriting of the person's SQL.** Changes their statement, breaks on `ORDER BY`
  without a key and on statements we cannot parse; rejected.
- **`DECLARE … CURSOR WITH HOLD`.** Materialises the whole result on the server at commit; rejected
  for large results.
