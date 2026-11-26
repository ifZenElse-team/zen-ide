# ADR-0283: A caller's symbol wait is honoured after a server has loaded

- **Status:** Proposed
- **Date:** 2026-11-11
- **Supersedes:** decision 3 of [ADR-0092](0092-places-by-line-and-editing-through-the-ide.md)
  (`SearchWait`), for searches the caller gave a `waitMs`

## Context

ADR-0092 made a symbol search wait while it is pending: up to its `waitMs`, up to 120 s while a
language server loads the project (`scanning`, `starting`, `indexing`), and otherwise up to 10 s
counted from the end of the last load. The 10 s kept an answer coming when a server is slow but
not loading.

In round six of the agents' dogfood, an agent called `navigate.symbol.goTo` with
`waitMs: 120000` and was answered `pending, reason: searching` after 14 s: rust-analyzer had
finished loading, the 10 s had passed, and the agent had to ask again for an answer it had already
said it would wait for. The agent chose its wait; the IDE cut it short.

## Decision

1. **An explicit `waitMs` is honoured.** When the caller gives `waitMs`, `navigate.symbol.goTo`,
   `navigate.class.goTo` and an IDE answering an agent's long wait (`IdeApp::waiting_lookups`) keep
   waiting while the search is pending, loading or not, until the answer comes or `waitMs` ends
   (`SearchWait::explicit`).
2. **The cap of 120 s stays** (`MAX_LOADING_WAIT`): no call waits longer, whatever `waitMs` says.
3. **Without `waitMs` the rule of ADR-0092 stays.** The IDE's own searches (Find Usages by symbol
   name) keep the 10 s after a load.

## Consequences

- One call with a long `waitMs` gives an agent the answer once the server has it, instead of a
  `pending` answer it has to ask again.
- A slow, idle server holds the call for the whole `waitMs`, up to 120 s; an agent that wants a
  quick answer gives a short `waitMs` or none.
- `a_query_asked_again_after_loading_gets_its_own_wait` in `language/symbols.rs` checks both rules.
