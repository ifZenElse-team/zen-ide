# ADR-0105: Search Everywhere's Text tab: Go to Text, its switches and file mask, and Text on All

- **Status:** Proposed
- **Date:** 2026-10-22
- **Amends:** [ADR-0081](0081-search-everywhere-contributors.md), [ADR-0095](0095-search-everywhere-toolbar.md)
- **Relates to:** [ADR-0033](0033-ai-access-to-every-api.md) (AI access),
  [ADR-0083](0083-search-scopes.md) (scopes)

## Context

The behaviour to build: Navigate → Text… (⌥⇧⌘E) opens Search Everywhere on its Text tab: the field has
Cc, W and .*, the toolbar a scope, an editable file type combo whose history is the one Find in
Files keeps for its file mask, Preview and Open in Find Tool Window; each row is the line with the
match marked and the file and line on the right. On All, text results come at the bottom and only
when the other groups found few results, which Advanced Settings can turn off. ADR-0095 gave us a Text tab
with the scope only, searched with Find in Files' engine, and kept text off All.

## Decision

1. **A contributor can be a fallback.** `SearchContributor::fallback` (false by default) says All
   searches it only once every other contributor of the query is done and they hold fewer than
   `FEW` (6) items together, and lists its group after every other group; otherwise it is marked
   done with nothing. The platform's Text contributor is the one fallback. The session setting
   `search.everywhere.textOnAll` (Advanced Settings → Search Everywhere, `searchTextOnAll`, on by
   default) leaves fallbacks out of All; Text is in All's Filter like the other groups.
   `search.everywhere.contributors` tells `fallback`.
2. **Text matches with options the query carries.** `zen_modules::search::TextOptions` (match case,
   whole words, regex, mask) is part of `SearchQuery` and of the run's key, so changing a switch or
   the mask starts a new search. The popup keeps them in its options (`search-everywhere.json`:
   `textMatchCase`, `textWholeWords`, `textRegex`, `textMask`, changed by
   `search.everywhere.setOptions`); `search.everywhere.query` takes `matchCase`, `wholeWords`,
   `regex` and `mask`, each falling back to the popup's choice, so an agent sees what the popup
   lists unless it asks otherwise.
3. **One mask history.** The file masks used go to `zen_workbench::find_history::FindHistory`
   (`find-history.json`, `search.findHistory.list` / `.add` / `.clear`), the history the find bar
   and Find in Files keep for their queries, replacements and masks. The Text tab adds its mask when
   a text result is chosen or the tab goes to the Find tool window, and its combo lists the masks
   of that history; Find in Files adds its mask the same way. The module is the one of another
   change, word for word, so that both changes can be merged in either order.
4. **The engine stays Find in Files'.** The Text contributor starts `search::spawn` (the
   project's text index, open files in their unsaved text) with the scope, the options and a limit
   of four matches per row asked (at most 2000), and cancels its search when the query, the options,
   the scope or the limit change; a new query, and closing the popup, also drop the run's
   contributors, which cancels the search still running. A row is one line, every match on it marked, `file:line` on the right.
5. **Go to Text is a window action.** `view.gotoText.open` (⌥⇧⌘E, Navigate → Text…) opens the
   popup on Text, or closes it when it is open there; `run_action` runs it.

## Consequences

- All may grow once more after it is shown: the Text group comes when the slower contributors are
  done. Text never holds All back, since it is not primary.
- The popup is as wide as its tabs and toolbar need (up to the window), so the Russian "Include
  non-project items" and the Text tab's scope and mask fit beside the tabs. In a window too narrow
  for that, the mask gives way first (down to 64 points), then the scope's name is cut short; the
  toolbar never covers the tabs.
- Merging with that other change (either order). `crates/platform/workbench/src/find_history.rs`
  is byte for byte that change's file, and the lines that use it are the same lines in the same
  places: `pub mod find_history;` and `find_history::register(registry)?;` in
  `crates/platform/workbench/src/lib.rs`; the `find_history` field of `Shared`, its load from
  `find-history.json` and `session.set_state(self.shared.find_history.clone())` in
  `crates/platform/ui/src/ide/app.rs`; the `search.findHistory.*` arm of `command_ui`; and the three
  `command-search-findHistory-*` lines of both `commands.ftl`, before
  `command-search-history-clear`. Git takes each once (checked with `git merge-tree`), so nothing of
  the module or its registration is dropped by hand. What is resolved by hand: in
  `find_in_files.rs` keep the other change's popup, which keeps its mask history itself, and drop this change's
  mask ⌄ and its `search.findHistory.add`; in the Advanced Settings (`settings/model.rs`,
  `editor.rs`, `page.rs`, `ui.ftl`) keep both changes; in `search_everywhere.rs` keep the other change's
  resizable popup (`popup_size`) and use this change's `width()` as its default width; the
  changelogs, `automation.md` and the guide keep both texts.

## Alternatives considered

- **Text on All as an ordinary contributor with a small weight.** It would search text on every
  keystroke on All and push its rows among the others; text is meant only as a last resort.
- **A mask history of Search Everywhere's own.** It would differ from Find in Files'; the two share
  one history.
- **Options only in the request.** Agents would have to repeat the popup's switches to see what it
  lists; reading the popup's options when a request leaves them out keeps the two the same.
