# ADR-0177: Crash-safe journals for the database workspace: checked records, one recovery journal per process

- **Status:** Proposed
- **Date:** 2026-10-27
- **Relates to:** [ADR-0084](0084-headless-ide.md) (an IDE without windows for agents)

## Context

A crash, a forced quit or a power loss must not lose the text of unsaved consoles, the data edits
staged over results, or the knowledge that a transaction was open (the server rolled it back). The
query history (JSON lines) was appended without a sync, so a power loss could tear its last line or
lose its tail, and a torn line could not be told from a damaged one.

The data folder of a product is shared by its IDE and by a headless one started for agents
(`<ide> mcp --headless`, the way agents work on this repository), so two processes can write the
same folder at the same time.

## Decision

1. **Journal records carry a check.** A journal line is `<crc32 in 8 hex digits> <json>\n`
   (`zen_db::journal`). A line whose CRC does not match, or that does not end, is dropped and never
   read; plain JSON lines written before (the earlier query history) are still read and rewritten with
   a check. Appends are synced at most once a second (a full sync on macOS); whole files are
   replaced through a temporary file, a rename and a sync of the folder. The query history
   (`history.jsonl`) uses this format from now on.
2. **One recovery journal per process.** What must survive a crash is kept in
   `<data folder>/databases/recovery-<process>-<n>.jsonl`, owner-only, as records `{ key, item }`
   (no item: removed): a console's text (`console:<path>`), a result's staged edits with the values
   their rows were read with and their keys, and an open transaction's counts and last statements'
   first lines. Passwords never enter it; statements are masked as in the history. While the
   journal is open its process holds `recovery-<process>-<n>.lock` locked (`File::try_lock`).
3. **Only journals of runs that stopped are taken over.** At start a journal reads those whose lock
   it can take — their process is gone — and the `recovery.jsonl` of earlier builds; their items
   become its own, are offered back for review, and their files are removed once written to its
   own. A journal another running process holds is never read, offered, compacted or removed.
4. **Nothing recovered runs by itself.** Recovered edits open as SQL in a new console, recovered
   text goes back into its console, and a transaction is reported as rolled back; the person
   decides what to run.

## Consequences

- An IDE and a headless IDE on one data folder keep separate journals; neither offers the other's
  live work as "recovered", and neither loses records by writing to a file the other replaced.
- **Downgrade:** a build from before this decision reads a checked history line as damaged and
  rewrites `history.jsonl` with the good lines only, so running an older build once empties the
  query history. It does not read per-process recovery journals at all.
- The history file itself is still shared by the processes of one data folder; a rewrite by one
  can lose lines the other appends until it opens the file again (docs/database-workspace.md).
- A run that stopped leaves its journal and lock file until the next start takes them over.

## Alternatives considered

- **One shared journal for the data folder.** Two processes then read each other's live items as
  recovered and compact the file under each other's open appends.
- **A journal per window.** Windows of one process share connections and consoles; one journal per
  process keeps one writer thread and one file.
- **SQLite as the store.** It would bring a C library into the default build ([ADR-0002](0002-open-source-clean-room-pure-rust.md)).
