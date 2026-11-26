# ADR-0192: The Database Navigator opens Properties and changes objects at once

- **Status:** Accepted
- **Date:** 2026-10-28
- **Supersedes:** the row "Opening a table" of ADR-0166, which is not part of this repository,
  for the Database Navigator
- **Relates to:** [ADR-0172](0172-production-safety-and-agents-on-databases.md) (production
  safety), [ADR-0176](0176-structure-scripts-and-migration-files.md) (structure scripts)
- **Decided by:** the owner, 2026-10-28

## Context

ADR-0166 decided how the Database workspace behaves. One of its decisions was that a table opens
on its Data page, not on its Properties. In the
hands-on test of 2026-10-28 this was the first thing that stood in the way, and the navigator also
had no Delete, Rename, Create or Truncate: changing an object meant opening its tab and staging the
change, or writing SQL.

## Decision

1. **A double click, ↩ or F4 on an object in the navigator opens its tab on Properties**.
   A table's tab lists its pages as Properties, Data, DDL and Diagram, so its rows are one
   page away; View Data in the menu opens them directly. `db.object.open` without `page` still opens
   tables and views on Data, so that agents and Search Everywhere keep their behaviour.
2. **Delete, Rename and Truncate from the navigator run at once**, not staged on a
   tab. They write the statement with the same script writer as staged structure changes and run
   it through the same path: the person's action in the Main context, production safety, and only
   the changed schema read again. A drop or truncate always asks first and names what is lost; a
   rename asks only where the connection type confirms changes.
3. **Create ▸ goes through the staged script.** It opens the schema's or table's tab with the form of
   the new object, so its script is seen before it runs, as with every other structure change.
4. **The tree stays flat:** no Databases or Schemas grouping folders.

## Consequences

- The Properties page and the Delete, Rename, Create and Truncate items are reached from the
  navigator itself.
- Dropping from the tree is one dialog instead of a tab, a staged change and Save; the dialog names
  the rows, size, columns, indexes and dependants lost and shows the statement, so the shortcut does
  not hide what it does.
- Open tabs of a dropped object close, and those of a renamed object open again under its new name.
