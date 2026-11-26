# ADR-0078: Messages to language servers are written by a thread of their own

- **Status:** Proposed
- **Date:** 2026-10-20
- **Relates to:** [ADR-0003](0003-hybrid-language-intelligence.md) (the semantic layer),
  [ADR-0077](0077-incremental-highlighting-with-line-lexers.md) (typing latency), docs/performance.md

## Context

`zen_lsp::Connection` wrote every message to the server's standard input on the caller's thread,
under a lock. The workbench sends a project window's changes, saves and questions (inlay hints,
semantic tokens, completion) from `language.servers.sync`, which the window runs every frame; the
thread reading the server also wrote its answers to the server's own requests through the same
lock. When a server is busy it stops reading, the pipe fills, and `write` blocks: a sample of
GoGarden while typing in a 10 000-line Go file showed the window waiting there, its p95 frame time
growing from 1 ms to 50 ms over 30 s of typing. A server that never reads again would freeze the
window for good, and the reading thread too, which could deadlock both pipes.

## Decision

1. **A writer thread per connection.** `zen_lsp::outbox::Outbox` holds a queue of messages that a
   thread serializes and writes to the server's input (buffered, one flush per message). Sending
   (`request`, `notify`, `respond`, `cancel`, and the reading thread's answers) only queues and
   returns; nothing in the window waits for a server to read. Messages are written in the order
   they were queued; the trace records them as they are written.
2. **What waits is merged, never reordered past something it depends on.**
   - A `textDocument/didChange` behind an unwritten `didChange` of the same document joins it: the
     changes of both, in order, at the newer version, from the last whole-text change on.
   - A request cancelled while still queued is taken out (`Outbox::withdraw`); the server sees
     neither it nor a `$/cancelRequest`, and the request answers `RequestCancelled` at once. The
     messages on either side of it may then merge.
   - A change, a save, a change and a save of the same document become a change and a save: the
     earlier save is superseded by the later one.
3. **Memory is bounded.** At most 64 MB (estimated from the messages' text) may wait. A server that
   lets the queue fill up has stopped reading and is treated as stopped: the queue is dropped,
   the connection is no longer alive, the requests waiting fail with `Closed`, and the client
   restarts it as after a crash (three times in three minutes at most). A failed write does the
   same.
4. **Dropping the connection closes the server's input** once what is queued is written; the
   reading thread holds only a weak reference to the queue.
5. `lsp.servers.list` reports each server's `queued` messages, so agents can see a server falling
   behind.

## Consequences

- The window's frame time no longer depends on how fast a server reads: on the 10 000-line typing
  benchmark with gopls the p95 stays at 0.8 ms (docs/performance.md).
- A message's failure to be written is no longer an error of the call that sent it; it shows as a
  stopped connection on the next `poll`, which restarts the server.
- Tests that assert a `$/cancelRequest` on the wire wait until the server has the request, since a
  request still queued is withdrawn instead.
