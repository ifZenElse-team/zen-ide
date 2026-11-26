# ADR-0118: Find Usages lists its usages on the Find tool window's tabs of occurrences

- **Status:** Proposed
- **Date:** 2026-10-22
- **Relates to:** [ADR-0103](0103-find-window-occurrences.md), [ADR-0007](0007-everything-is-a-command.md),
  [ADR-0033](0033-ai-access-to-every-api.md)

## Context

Find Usages (⌥F7) kept its usages in a state of its own (`navigate.usages.list`) that the Find tool
window drew as a second, flat tree of files and lines, with no Group By, no filters, no preview, no
pin and no rerun. The search bar's Open Results in Find Window used the same flat tree. The
requirement: the usages of a symbol are shown on a tab of the Find
tool window built like the tab of Find in Files' occurrences: the same Group By (Usage Type, Module,
Directory Structure…, Merge Usages from the Same Line), the read and write filters, Preview Source,
Pin Tab, Rerun and Open Results in New Tab; Edit → Find Usages has Recent Find Usages, and
⌥⇧⌘F7 opens the Find Usages dialog with the scope and "Skip results tab with one usage".

## Decision

1. **One tab model for text and code** (`zen_workbench::find_results`): a tab is of a kind
   (`text`, `usages`, `file`, `search.results.list`), and every kind is the same list of usages
   under the same computed tree, Group By, exclusion, selection, pin and commands
   (`search.results.*`). The Find tool window has one tree implementation; the old flat usages tree
   is gone. Find Usages, Show Usages' Open in Find Tool Window and the search bar's Open Results in
   Find Window all make such tabs.
2. **A Find Usages tab is made when the server answers.** `navigate.usages.find` asks
   `textDocument/references` as before, shows a background task "Finding usages of '…'" and,
   with the answer, places the tab as Find in Files places its tabs (in place of the last tab that
   is not pinned, unless Open in new tab), or with Skip results tab with one usage opens a single
   usage in the editor instead. Rerun asks the server again for the tab's target, in place.
   `navigate.usages.list` stays and tells the tab (`tab`).
3. **Read and write come from the server only.** For each file with usages (at most 500) the
   workbench asks `textDocument/documentHighlight` at the file's first usage in code, in the
   background: the tab is made at once, at most 8 requests wait at a time, each is given up after
   5 s, the files lent to the server stay open only while their request waits, and closing,
   stopping or rerunning the tab cancels the requests (`accessPending` on the tab until it is
   over); a usage the server calls Read or Write goes
   under "Value read" or "Value write" and Show Read Access and Show Write Access
   (`search.results.setOption` `showRead`, `showWrite`) filter it. A usage the server does not
   classify stays Unclassified and is never filtered; the filters are shown only on a tab where the
   server told some access (`accessKnown`). Imports and comments are told from the text as Show
   Usages tells them.
4. **The options are the window's**: `navigate.usages.options` keeps Scope (Project Files, All
   Places, Open Files, Current File), Open in new tab and Skip results tab with one usage. The Find
   Usages Settings dialog (`navigate.usages.settings`, ⌥⇧⌘F7) edits a draft that Find keeps
   (`navigate.usagesSettings.get/set/find/cancel`); opened over the Show Usages popup it closes the
   popup and Find shows the popup again in the chosen scope. The popup's own settings panel stays
   reachable through `navigate.showUsages.setOptions` `settings`.
5. **Recent Find Usages** is the window's last 15 Find Usages (`navigate.usages.recent`), newest
   first, kept in memory for the window; `navigate.usages.openRecent` runs one again. If the file
   changed, it and Rerun look for the symbol on a line with the text of the line it was on
   (nearest to its column), else nearest to where it was, so a symbol of the same name nearer to
   the old offset is not taken instead. The main menu has a static Edit → Find Usages submenu, so Recent Find
   Usages is an item that opens a popup of the searches (`view.recentFindUsages.open`) rather than
   a submenu of them.

## Consequences

- The Find tool window's toolbar, keys and menus are the same on every tab; agents read code
  usages with `search.results.tree` as they read text occurrences.
- Find Usages asks the server one more request per file; a server without document highlights is
  not asked, and its usages stay Unclassified.
- Our own choices, still to be checked against the UI specification: the labels "Usages in <scope>",
  "Value read", "Value write", the order of the usage types, the defaults of the dialog's check
  boxes, the number of recent searches, and a tab that says why nothing was found instead of a
  balloon.
