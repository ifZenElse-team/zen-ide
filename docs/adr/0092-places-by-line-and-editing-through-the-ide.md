# ADR-0092: Places by line and column, project-relative symbols, the end of a load waited for, a missing server named once, and editing through the IDE

- **Status:** Proposed
- **Date:** 2026-10-22
- **Extends:** [ADR-0090](0090-one-based-places-and-lean-test-results.md) (one-based places) and
  [ADR-0091](0091-agent-answers-round-three-and-command-tools.md) (round three); part of developing
  Zen IDE through RustRonin

## Context

The round-three dogfood and this round's, both through a headless RustRonin on this
repository, found what still made agents work around the IDE:

- **Offsets only.** Every answer gives a place as a one-based line and column (ADR-0090), but Find
  Usages, Show Usages, Go to Declaration and Rename took a character offset only: an agent had to
  read the file and count characters itself.
- **Absolute symbol paths.** Search and usages answers name files relative to the project; Go to
  Symbol and Go to Class still gave each symbol's absolute path, 120 bytes of it in this worktree.
- **`searching` with the server idle.** On this repository rust-analyzer loads for about 13 s. A
  query answered during the load is asked again when the load ends (its answer may be incomplete);
  that second answer comes in milliseconds, but by then the search had waited more than its 10 s
  (`MAX_WAIT`) and answered `pending, reason: searching` at once. RustRonin 0.2.6 answered
  `searching` after 13 s and again after 25 s; the third call had the symbol: 38 s and three calls.
- **The missing server on every search.** Docker, YAML and TOML files of the repository have no
  server installed here; every symbol search, whatever it found, carried 270 bytes naming the
  Docker server, which no Rust name matches.
- **Scripts instead of the IDE.** Round three's agent edited with Python scripts. Measured by
  making this round's change through `editor.text.replace`: 32 file edits in 15 change sets took
  38 calls, one per file; a new file could not be given its text (`oldText` may not be empty and
  `file.fs.newFile` creates an empty file, its folder given as an absolute path only); a pattern
  (every `LookupParams { path, offset, wait_ms }`) needed one exact edit per place; and one file was
  rewritten whole, which needs its whole old text. Edits made by scripts bypass undo, Local History
  and the editor's view of the file.

## Decision

1. **`line` and `column`** (one-based, the column in characters, 1 when absent; a column past the
   line's end is its end) are accepted wherever a place is: `LookupParams` (Go to Declaration,
   Quick Documentation, peek, Prepare Rename...), `navigate.usages.find`, `navigate.usages.show`,
   `editor.highlights.get`, `refactor.rename.preview` and `refactor.rename.apply`. An `offset`, when
   given, wins. A line the file does not have is an error naming the file (relative) and its number
   of lines.
2. **Symbol and type paths are project-relative**: `navigate.symbol.goTo` and `navigate.class.goTo`
   answer `path` relative to the project with `/` (through a link to the project too), absolute
   outside it. The IDE's own views keep the absolute path (`SymbolItem::path`, not serialised).
3. **`SearchWait`** replaces `keeps_waiting`: a search keeps waiting while it is pending, up to its
   `waitMs`, up to 120 s while a server loads, and otherwise up to 10 s counted from the moment the
   last load ended (from the start when nothing loaded). The re-asked query is answered within its
   own wait; the cap of 120 s from the start and `waitMs` still hold.
4. **A missing server beside found symbols is named once per session** per server
   (`Languages::told_missing`); an answer with nothing names it every time, since it may be why.
   We considered naming it only when the query could match the server's files, but a name query
   says nothing about which language holds the name; once per session is simple and measured.
5. **Editing through the IDE**: `editor.text.replace` and its `edits` take `regex` (the search's
   own matcher: `^`/`$` at line ends, case kept, groups as Replace in Files fills them, error
   `invalid_regex`); `editor.text.write { path, text, save? }` gives a file its whole text, creating
   a file of the project with its folders; `editor.text.batch { files, save? }` takes for each file
   the replace parameters or a whole `text`, checks every file first and changes all or none (an
   error names the file by index, `file`); the `file.fs.*` commands and `project.file.paste` take
   paths relative to the project. `editor_text_write` and `editor_text_batch` are MCP tools of their
   own, and the server's instructions tell agents to edit through the IDE rather than with scripts.

## Consequences

- An agent can go from any answer (search, problems, a test failure) to usages or a rename without
  reading the file to count characters.
- The first Go to Symbol on this repository is one call of 12 s instead of three calls and 38 s;
  a symbol answer is 242 bytes instead of 605.
- This round's edits would take 21 calls instead of 38 with `editor.text.batch`; new files and
  pattern changes need no script. `tools/list` grows by the two tools' schemas.
- A batch that fails after some files changed (a write that fails) takes back the files changed
  before it (undo, or removing a created file); the check before any change makes that rare.
- Still open, for a later round: `refactor.rename.preview` and `navigate.declaration.goTo` repeat
  absolute paths per change and target; `file.fs.delete` answers absolute paths; a Rust file edited
  through the IDE can stay `notChecked` until rust-analyzer next publishes its diagnostics.
