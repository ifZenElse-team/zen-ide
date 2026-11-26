# ADR-0103: The Find tool window's occurrences are a background search kept by the workbench; every replacement goes through one guarded engine

- **Status:** Proposed
- **Date:** 2026-10-22
- **Relates to:** [ADR-0007](0007-everything-is-a-command.md), [ADR-0033](0033-ai-access-to-every-api.md),
  [ADR-0082](0082-trigram-index-for-find-in-files.md)

## Context

Find in Files' **Open in Find Window** searched synchronously, cut the matches at the Too Many
Results limit and handed them to the Find Usages state as a flat list of files. The specified
behaviour: the search runs in the background with "Searching for '…' in
<file>…" and a cancel button in the status bar, pauses at the limit to ask Abort / Continue, and
shows the results as a tab "Occurrences of '…'" with Targets, "Found occurrences in Project",
usage types, modules, compacted folders, files and lines, the Group By options, Exclude and
Include, and for Replace in Files Replace Selected and Do Replace All. Agents must reach all of
it (ADR-0033), and replacing must never lose a user's text.

## Decision

1. **The workbench keeps the tabs** (`zen_workbench::find_results::FindResults`): each tab holds
   its search parameters, the replacement of a Replace in Files tab, the matches found so far (each
   with its usage type, from the highlighter's tokens, and the declarations holding it, from the
   outliners), the excluded and the replaced usages, the collapsed nodes, the selection and the pin.
   The window draws what `search.results.tree` gives; every action of the tab is a
   `search.results.*` command (tree, list, tooMany, stop, rerun, setOption, exclude, include,
   select, next, previous, expand, pin, close, replace).
2. **The search runs in the background as a cancellable task.** `Tasks` gets `cancel_with` and
   `tasks.background.cancel`; the status bar shows a × for such a task. The index tells the search
   which file it reads (`FileIndex::search_text_visiting`) for the status bar's "in <file>…". At the
   limit (`search.findInFiles.tooManyResults`) the workers wait until Continue or Abort; cancelling
   ends the wait.
3. **The tree is computed, not stored**: from the matches and the Group By options (one session
   setting, `search.results.options`), so changing an option regroups at once. A node's id is the
   path of its groups; exclusion is kept per usage, so it survives regrouping.
4. **One replacement engine** (`search::replace_matches`) serves Replace in Files, Replace
   Selected and Do Replace All: all or nothing; it refuses a read-only file, a file whose project
   settings give a charset other than UTF-8 or whose bytes are not UTF-8, a file outside the
   project, and a file whose matched line changed since the search; the files on disk are written
   first, and one that cannot be written puts back those written before it, so the open files
   change only when every other file is written; an open file changes in its editor as one undo
   step, any other is written atomically with a Local History version from before; inserted line
   breaks follow the file's CRLF. After Replace Selected the tab's remaining usages are moved to
   where the replacement put them, so usages of one file can be replaced one by one; replacing
   waits for the search to end.
6. **The tab's shortcuts stay in the tree**, as in the other tool windows (Find Usages, Structure,
   Debug): ⌘R, ⌥⌘↑/↓, ⇧⌘F, ⌘+/⌘−, ⌃D ⌃S ⌃M ⌃O ⌃P ⌃T ⌃L and ⌫ act only while the tree has the
   keyboard, where they go before the keymap; elsewhere the keymap's actions keep them. The keymap
   has no tool-window context, and every one of these actions is a `search.results.*` command, so
   agents and the Search Everywhere actions reach them without a shortcut.
5. **File Structure grouping uses the syntax outliners only** (no language server request in the
   background), so files without an outliner have no declaration level.

## Consequences

- `search.text.occurrences` answers at once with the tab (`tab`, kept as `search` too) instead of
  the final count; agents wait with `waitMs` or read `search.results.tree`. Find bar's ⌥F7 keeps
  the Find Usages state.
- Replace in Files now refuses read-only and non-UTF-8 files instead of skipping or failing midway.
- The behaviour the UI specification does not settle yet (the tab title of Replace in Files, ⌫ on
  an excluded node including it back, the exact labels of the counts) is provisional.
