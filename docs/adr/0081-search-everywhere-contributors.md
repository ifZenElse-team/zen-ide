# ADR-0081: Search Everywhere contributors, streaming results and holding them back

- **Status:** Proposed
- **Date:** 2026-10-20
- **Relates to:** [ADR-0007](0007-everything-is-a-command.md) (everything is a command),
  [ADR-0031](0031-plugins.md) and [ADR-0071](0071-built-in-modules.md) (plug-ins and built-in
  modules), [ADR-0033](0033-ai-access-to-every-api.md) (AI access)

## Context

Search Everywhere had its four tabs written into the popup: each frame the popup dispatched
`navigate.class.goTo`, `navigate.file.goTo`, `navigate.symbol.goTo` and searched the actions
itself. Nothing else could add a tab, agents could not ask for the list the popup shows, the list
changed under the user as slower sources answered, and the recent actions were a static list lost
at every restart. The popup is to be built from contributors that stream weighted items with a
cancellation token, and to hold the results back for a short time so that the list does not jump.

## Decision

1. **A contributor is an interface of the module host** (`zen_modules::search::SearchContributor`,
   layer L4, so that built-in modules can implement it): an `id` (also its tab's id), the Fluent id
   of its `title`, whether it `has_tab`, a `weight` (its tab and its group in All go lighter
   first), whether it is `primary`, and `search(session, query, sink)`, which pushes items into a
   `SearchSink` (it refuses more once the limit is reached or the query was cancelled) and answers
   `Done` or `Pending(what it waits for)`. The query carries the text, whether it is for All,
   whether non-project items are wanted, the limit and a `CancelToken`, set when a newer query
   replaces it. A module adds one with `ModuleHost::search_contributor`; a window's session holds
   the others in `SearchContributors`.
2. **Items are data, not presentation** (`SearchItem`): a name, a detail, a hint, whether the item
   is in the project and available, and a target (`file`, `place` for `navigate.place.goTo`, or
   `action` for `run_action`). The popup draws any contributor's items the same way, and the items
   serialise as they are.
3. **The platform's contributors** are Classes (100), Files (200) and Symbols (300, not primary) in
   `zen_workbench::search_everywhere`; Actions (400) is in `zen-ui`, which knows the keymap.
4. **Streaming and holding back** (`SearchRun`): each poll asks only the contributors that are not
   done, and for the same query at most every 50 ms, so that frames drawn for other reasons do not
   search again; a new query cancels the last one and starts again. The new results replace the list shown
   only when every primary contributor of the tab (on a tab of its own, its contributor) is done or
   has 8 items (the limit, when it is lower), or after 2000 ms; until then the last query's list
   stays. Advanced Settings → Search Everywhere → **Wait for all contributors to finish**
   (`searchWaitForAll`, the session setting `search.everywhere.waitForAll`) holds them until every
   contributor is done, with no time limit.
5. **One answer for people and agents.** `search.everywhere.query { tab, query, includeNonProject,
   limit?, waitMs? }` runs the same search as the popup and answers its groups;
   `search.everywhere.contributors` lists the tabs. Non-project items (libraries, the SDK) are
   left out unless asked for ("Include non-project items", or ⇧⇧ again in the open popup). The
   command waits while the results are held back, so that it answers the list the popup settles on
   rather than the last query's.
6. **History across restarts.** The queries a result was chosen with (50) and the actions run from
   the popup (20) are kept in `search-everywhere.json` in the IDE's data folder, shared by the
   windows; `search.history.list` and `search.history.clear` read and clear them, and ⌥↓ / ⌥↑ bring
   the queries back in the field.

## Consequences

- WebAssembly plug-ins cannot add a contributor yet: the ABI (docs/plugins.md) has no export for
  it. A later ABI version can wrap one as a `SearchContributor` without changing the popup.
- Classes and Symbols no longer list library and SDK declarations by default; the checkbox shows
  them.
- A contributor runs on the window's thread, as the commands it wraps do; a slow one must keep its
  work in the background and answer `Pending`, as the language servers' search already does.
