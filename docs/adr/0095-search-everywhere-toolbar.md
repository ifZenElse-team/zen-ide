# ADR-0095: Search Everywhere's toolbar, options and the Text and Recent Files contributors

- **Status:** Proposed
- **Date:** 2026-10-21
- **Amends:** [ADR-0081](0081-search-everywhere-contributors.md) (decisions 2, 3 and 6)
- **Relates to:** [ADR-0033](0033-ai-access-to-every-api.md) (AI access),
  [ADR-0083](0083-search-scopes.md) (search scopes)

## Context

Search Everywhere is to name the type tab **Types** in a Go project (Navigate → Type… ⌘O), to have a
**Text** tab (⌥⇧⌘E), and to give every tab a toolbar: Include non-project items or the scope,
Preview, Filter and Open in Find Tool Window on All, Types, Files, Symbols and Text, and Include
disabled actions on Actions. With nothing typed, All lists the recent files and the other tabs
nothing; the footer shows the selected item's path and Open In Right Split. Our popup had none of
it, and its options would have to be remembered and reachable by agents.

## Decision

1. **The type tab is named by the language.** `zen_modules::search::TypeNoun` (`Class`, the
   default, or `Type`) is a window-session state a built-in module sets with `window_state`; the Go
   module sets `Type`. The Classes contributor keeps the id `classes` (agents and keymaps use it)
   and takes its title from it (`ide-se-classes` or `ide-se-types`), and the main menu names
   `view.gotoClass.open` Type… when it is `Type`.
2. **Two more platform contributors.** Recent Files (`recentFiles`, weight 50, no tab) lists the
   recent files but the current one, only on All and only while nothing is typed; Text (`text`,
   500, not primary) runs Find in Files' search in the background (`search::spawn`) for the query
   and the tab's scope and lists each matching line once, on its own tab only: All does not search
   text, and its Filter does not list Text, until the Text tab is completed on its own. With
   nothing typed every other contributor lists nothing. Files also lists the folders whose name matches, as a new target
   kind, `directory`, which selects the folder in the Project tool window.
3. **The query carries the scope** (`SearchQuery::scope`), so that a contributor that searches
   itself (Text) keeps to it instead of being filtered afterwards.
4. **The popup's options are kept with the history.** `EverywhereOptions` (Preview and its height,
   the contributors All leaves out, the file types Files leaves out, the languages Types and Symbols
   leave out, Include disabled actions) live in `search-everywhere.json` next to the queries and
   actions, under `options`; a file without it reads as the defaults. The search applies them, so
   the popup and `search.everywhere.query` list the same items. The Filter's entries come from one
   function (`filter_entries`): the contributors on All, `directory` and then the file types by
   name on Files, the languages of the language servers on Types and Symbols.
5. **Every control is a command.** `search.everywhere.options` and `search.everywhere.setOptions`
   read and change the options, `search.everywhere.filters { tab }` lists a Filter with what is
   ticked, and `search.everywhere.openInFind` searches and puts the results on a new tab of the Find
   tool window (refused on Actions, as the button is off there). The popup's button puts the list
   it shows there without searching again (`show_in_find`).

## Consequences

- Actions that cannot run now are left out of Actions and All unless Include disabled actions is
  ticked; before, they were listed dimmed.
- The file types of the Filter are a fixed table in the workbench; a plug-in cannot add one yet.
- The Text tab has no shortcut (⌥⇧⌘E, Navigate → Text…), no file mask combo and no Cc, W and .*
  yet, and its lines are not on All; `/` commands are not listed yet, although the field's
  placeholder names them.
