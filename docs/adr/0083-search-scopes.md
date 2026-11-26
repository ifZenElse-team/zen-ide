# ADR-0083: Search scopes, their pattern language and the project's own named scopes

- **Status:** Proposed
- **Date:** 2026-10-20
- **Relates to:** [ADR-0051](0051-jetbrains-keymaps-and-idea-project-settings.md) (`.idea` is read, never
  written), [ADR-0033](0033-ai-access-to-every-api.md) (every capability is a command)

## Context

Find in Files has a scope row (In Project, Module, Directory, Scope) and a list of
scopes shared with the tabs of Search Everywhere: predefined ones, one per module and the project's
named scopes, whose patterns are written in a small language (`file:src//*&&!file:*_test.go`,
`$Other`). We already read the named scopes of `.idea/scopes/*.xml` (ADR-0051) but did not use them,
and ADR-0051 forbids writing `.idea`, while people need to add and edit named scopes.

## Decision

1. **Scopes are ids.** `search.scope.list` lists them in this order: `allPlaces`,
   `projectFiles`, `projectAndLibraries`, `projectNotIgnored`, `gitTracked`, `scratches`,
   `recentlyViewed`, `recentlyChanged`, `module:<name>` for each module, `currentFile` (Find in
   Files only), `allChanged`, `production`, `tests`, then `named:<name>`. `search.text.start`,
   `.find` and `.replaceAll` take `scope` (Project Files when absent) with `directory` and
   `recursive`; `search.scope.files` and `search.scope.filter` say which files a scope holds.
2. **A scope is a filter over the project's relative paths plus folders outside the project**
   (`zen_index::FileFilter`, `ResolvedScope`): libraries for All Places and Project and Libraries,
   the scratches folder (`<data>/scratches`) for Scratches and Consoles. Replace in Files refuses
   files outside the project.
3. **The pattern language (`zen_index::scope`)** supports `file:` patterns (`*` within a folder,
   `?`, `//` for any folders below, a name pattern without `/` matches the file name, case
   ignored), `file[module]:` relative to the module's folder, `&&`, `||` (weaker), `!`, parentheses
   and `$Name`. Other kinds (`src:`, `lib:`, `test:`...) are refused with a message, as are unknown
   names and cycles; such a scope is listed with its error and cannot be chosen.
4. **Test files** are the names `*_test.go`, `*.test.*`, `*.spec.*`, `test_*.py`, `*_test.py`;
   Project Production Files are the others.
5. **The project's own named scopes live in `.zen/scopes.json`** (`{ "scopes": [{ "name", "pattern" }] }`),
   written by `search.scope.save` and `search.scope.delete` and the Scopes window. `.idea/scopes`
   stays read-only; an own scope of the same name hides it (as run configurations do, ADR-0051).
6. **The choice is remembered per project** (`search.scope.select`, saved in `workspace.json`):
   the area, module, folder, recursion and scope of Find in Files, and the scope of Search
   Everywhere, whose All tab shows it as "Include non-project items" (All Places or Project Files).

## Consequences

- Agents and people search the same scopes; a named scope from a JetBrains IDE works unchanged as
  long as it uses `file:` patterns.
- Search Everywhere filters the files, classes and symbols it found by the scope; files outside the
  project are not yet found by its Files tab.
- A later change can add the other pattern kinds without changing the ids or the file.
