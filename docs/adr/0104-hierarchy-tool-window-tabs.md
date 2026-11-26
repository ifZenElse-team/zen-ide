# ADR-0104: The Hierarchy tool window keeps one live hierarchy per tab

- **Status:** Proposed
- **Date:** 2026-10-22
- **Relates to:** [ADR-0007](0007-everything-is-a-command.md),
  [ADR-0033](0033-ai-access-to-every-api.md), [ADR-0055](0055-language-servers.md)

## Context

The call and type hierarchy commands (`navigate.hierarchy.*`) kept one hierarchy per window: a new
Call or Type Hierarchy replaced it and cancelled its requests. The Hierarchy tool window has tabs:
a new hierarchy replaces the selected tab unless it is pinned (Pin Tab), pinned tabs stay, and the
tabs switch by their titles. The review of those commands noted that pinned tabs need several
hierarchies at once. Two ways were open:

1. Show a pinned tab as a **snapshot**: the window copies the state when the tab is pinned and
   draws the copy. The API stays as it was, but a pinned tab is dead: its nodes no longer expand
   lazily, Refresh, Caller/Callee and Expand All do nothing there, and agents cannot read it.
2. Keep **one live hierarchy per tab** in the workbench, the commands addressing a tab.

## Decision

1. The workbench keeps the hierarchies of a window as **tabs** (`Hierarchies`): each tab is a whole
   hierarchy as before (its requests, nodes, scope, direction), with an id and a pinned flag; one
   tab is selected. `navigate.hierarchy.call` and `.type` replace the selected tab when it is not
   pinned, else another unpinned tab, else open a new one, and select it.
2. The commands that act on a hierarchy (`get`, `refresh`, `setOptions`, `cancel`, `close`) take an
   optional `tab`, the selected one by default, so every call made before tabs existed works the
   same. Node ids stay unique in the window across all tabs, so `expand` and `cancel` with a node
   find its tab themselves.
3. New commands, each the action of the tool window: `navigate.hierarchy.pin` (Pin Tab),
   `.select` (a click on a tab's title), `.collapse` (a node, or Collapse All), `.expandAll`
   (Expand All, down to a depth, 3 levels by default, at most 10, with at most 256 requests to the
   server, so a wide tree does not flood it). Sort Alphabetically and
   Navigate with Single Click are options of `setOptions`; the scope, sorting and single click are
   kept for the next hierarchy and saved with the project's layout (`workspace.json`).
4. A node that is one of its own ancestors (same name, file and place) is marked `recursive` and is
   never asked about, so a recursion ends in the tree and in Expand All.
5. The state answers with the list of tabs and the selected one; `Hierarchies::revision` changes
   with every command and with every answer of the server taken in, and the tool window reads the
   state again only then or while answers are awaited.

## Consequences

- A pinned tab stays fully alive: it expands, refreshes and turns the other way, and agents read
  and drive it by its `tab` id, as people do in the window.
- Requests of a tab that is not selected are taken in when that tab is read (`get` with its `tab`,
  or selecting it); a tab left alone waits with its answers.
- Only one tab's root request holds its file open in the server at a time (the language layer has
  one holder per feature); a root request of another tab is still answered, as the server got the
  file when the request was sent.
- Closing a tab cancels its requests and selects the tab before it, else the one after.
