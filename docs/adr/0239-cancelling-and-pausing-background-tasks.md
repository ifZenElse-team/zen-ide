# ADR-0239: Cancelling and pausing background tasks

- **Status:** Proposed
- **Date:** 2026-11-05

## Context

Background tasks (`zen_workbench::tasks`) could only be cancelled by a flag their work polled, and
indexing could be neither paused nor stopped. A killed program also reported an ordinary failure,
with an error notification. Language servers report work with `$/progress`, shown nowhere but in
`language.servers.status`.

## Decision

1. **A task owns how it stops.** `TaskHandle::on_cancel` adds a hook; `cancel_with` sets a flag;
   `cancel_kills` interrupts the task's process groups on a thread of its own and kills them 2
   seconds later, so that the UI thread never waits. A task with no hook has no ×.
2. **`cancelled` is a state of its own.** After a cancel, `finish` ends the task `cancelled` whatever
   its work reports: the error of a killed program is not an error to tell the user.
3. **Only what can stop cleanly is cancellable.** Git's fetch and push (Git removes its temporary
   packs on SIGINT), `go get`, whose `go.mod` and `go.sum` are restored from a copy, Maven and
   Gradle loads, file watchers and searches. A pull or an update is not: stopping it in the middle
   of a merge would leave the working tree half-way. Downloads are not yet: they read into memory
   and have no way to stop, and write nothing until they have everything.
4. **Pausing is a hook too** (`on_pause`, `pausable`, `paused`). The only pausable task is the first
   build of the text index, which holds its workers at a gate between two files. It has no ×:
   stopping it would leave an index that never gets ready. While paused it keeps what it has read,
   and the search reads the other files as it does during any build. Caffeine does not count a
   paused index as indexing. The gate opens when the index is dropped, so that a paused build
   never outlives its project.
5. **A language server's work is a task while it lasts.** `language.servers.sync` publishes each
   `$/progress` token as a task of source `language-server`, cancellable with
   `window/workDoneProgress/cancel` when the server says `cancellable`. Servers report work all the
   time, so such a task disappears when it ends instead of filling the list of finished tasks, and
   exiting does not ask about it.

## Consequences

- Agents have `tasks.background.pause`, `tasks.background.resume`, `tasks.panel.toggle` and the
  fields `cancelling`, `pausable` and `paused`.
- `FixState` has a variant `Cancelled`.
- Short work of a language server flashes in the status bar for as long as it lasts.
