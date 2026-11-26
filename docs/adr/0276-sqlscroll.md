# ADR-0276: SqlScroll, the IDE for SQL and databases

- **Status:** Proposed
- **Date:** 2026-11-11
- **Amends:** [ADR-0019](0019-alliterative-product-names.md) (the product takes its place in the list)
- **Extends:** [ADR-0050](0050-products-are-plugin-bundles.md), [ADR-0164](0164-database-engine-zen-db-runtime-and-drivers.md)
  (the database module is on by default here)

## Context

[ADR-0005](0005-naming.md) and [docs/naming.md](../naming.md) name SqlScroll as the family's IDE
for SQL and databases, with a scroll as its image ("a scroll keeps records"). Its tile and its Kozō
holding a scroll ([ADR-0125](0125-kozo-the-family-mascot.md)) are already drawn, and Zen Toolbox
lists it as coming soon. [ADR-0164](0164-database-engine-zen-db-runtime-and-drivers.md) made the
database engine a built-in module, on by default in SqlScroll and offered in the other IDEs, and
the Database workspace now exists in every
IDE. What was missing is the product itself.

## Decision

1. **The app.** `apps/sql-scroll` is SqlScroll as JvmJizo is JvmJizo
   ([ADR-0147](0147-jvmjizo-app-and-jdks.md)): product id and launcher `sqlscroll`, bundle id
   `space.ifzenelse.sqlscroll`, MCP port 63534, its tile as the icon (`#6FE0C8` and `#1C7F86`),
   its Kozō on the splash and welcome screens.
2. **Databases first.** The `database` module is on, so the **Database** button and workspace are
   there from the first start; Go and Rust, the language modules, are off and can be switched on in
   Settings → Plugins like in any IDE. Nothing connects by itself (AGENTS.md rule 10): the first
   connection is made when the person or an agent asks for it.
3. **The bundle** (`PRODUCT_PLUGINS`) is the base set ([ADR-0036](0036-bundled-plugins.md)) and the
   `sql` plug-in. The plug-ins of other database drivers (SQL Server, Oracle, DuckDB, MongoDB) come
   from the catalog, as in every IDE: they carry driver programs of their own, which the bundle does
   not build.
4. **Not released yet.** SqlScroll is in `release::UNRELEASED_IDES`: built and tested with the
   workspace, left out of `release-build ides` and `release-publish`. Its release is the owner's
   (AGENTS.md rule 12): it moves the line to `release::IDES` and starts `changelog/sqlscroll.md`,
   and Zen Toolbox then lists it as available once the signed manifest has it.
5. **Every capability stays a platform feature or a plug-in for every IDE** (ADR-0050): SqlScroll
   only chooses what is on. A database feature never lands in the app crate.

## Consequences

- People who work mostly with databases get an IDE where the Database workspace is the first thing
  they see; everyone else keeps the same workspace one switch away.
- The release is one line in `release::IDES` and a changelog file, done by the owner.
