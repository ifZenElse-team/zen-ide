# ADR-0139: Compact lists, problem filters, usages by name, edit reports, hidden files and call costs over MCP

- **Status:** Proposed
- **Date:** 2026-10-24
- **Extends:** [ADR-0087](0087-agent-sized-mcp-answers.md), [ADR-0089](0089-mcp-answers-once-and-in-english.md),
  [ADR-0090](0090-one-based-places-and-lean-test-results.md), [ADR-0091](0091-agent-answers-round-three-and-command-tools.md),
  [ADR-0092](0092-places-by-line-and-editing-through-the-ide.md), [ADR-0084](0084-headless-ide.md)

## Context

An agent's real task was measured through the MCP server against working without the
IDE. P0 (one-call edits, ranged reads, the catalogue on demand, errors instead of empty answers)
and the later rounds (answers once and in English, one-based places, lean test results, tools of
their own) are done. What was left made an agent still pay more through the IDE for some steps:

- **Lists of places were objects.** A symbol was `{name, qualifiedName, kind, container, path,
  offset, line, column, inProject, ranges}`, about 200 bytes; 15 symbols took 3 KB (a grep line is
  about 70 bytes).
- **Problems came all at once.** `problems.project.list` answered every problem of every file with
  offsets, popups and fixes; an agent wanting the compile errors only could not ask for them.
- **Usages needed a place.** `navigate.usages.find` took a path and an offset (or line and
  column); finding the usages of `Store.Get` took a symbol search first, and the places a server
  cannot see (a name in a string or a comment) needed a separate text search.
- **An edit was followed by a check.** After `editor.text.replace` an agent called
  `problems.file.list`, often too early (`checking`).
- **Secrets.** A headless IDE handed an agent `.env` and credential files on request, and its tabs
  came back from the last session.
- **No measure of cost.** Nothing said what a session's calls cost.

## Decision

1. **Compact lists** (`compact: true`): `navigate.symbol.goTo` and `navigate.class.goTo` answer
   `rows`, `path:line:column kind name` (a symbol's type, `in Store`, when the name does not say it
   and the container is not a package path); `editor.structure.get` answers `rows`
   `line:column kind name detail`, indented two spaces per level; `navigate.usages.find` answers
   `rows` `path:line:column text`. The object list is then empty. Without `compact` nothing
   changes. Shared helpers: `zen_workbench::language::compact`.
2. **Problem filters** on `problems.file.list` and `problems.project.list`: `severity` (the least
   severe kept: `error` keeps errors, `warning` errors and warnings), `sources` (ignoring case),
   `limit` (`truncated` counts what it left out) and `compact` (rows `line:column severity message
   [source code]`, the message's first line; `path:` before them in the project's list).
   `problems.project.list` also takes `paths` and `exclude`: patterns of project-relative paths
   with `*` (`crates/*/src/*.rs`), file names (`*.go`) or folders (`vendor`). A filtered answer is
   always `{problems, checked, reason?, truncated?}` (file) or `{problems, total, truncated?,
   reason?, server?...}` (project); without a filter the answers are as before.
3. **Usages by name.** `navigate.usages.find` takes `symbol` (a declaration by name as Go to Symbol
   takes it: the best match whose name is the query's last part, waiting up to `waitMs` for the
   servers; errors `noMatch` or `pending` with `symbol`) or `name` with `line` (the name's first
   whole-word occurrence on the line, or in the file without `line`; error `not_found`). With
   `waitMs` the usages were already in the answer; `includeTextMatches` adds `textMatches`: the
   whole-word, case-sensitive occurrences of the name in the project's files (Find in Files'
   search) that are neither a usage the server found nor the place searched from, as rows, at most
   200, once the usages have come. They are what a server cannot see: comments, strings, other
   languages, or other symbols of the same name.
4. **Edit reports.** `editor.text.replace`, `editor.text.write` and `editor.text.batch` take
   `checkProblems` (and `checkMs`, 3 000 by default, at most 10 000): after the edit the command
   syncs the file to its language server and waits until the server has checked the new text
   (`Languages::analysis`), then answers `newProblems`, the errors and warnings that were not there
   before (compared by severity, message, source and code, as many times as they occur), as rows; an
   empty list when it brought none. `unchecked` says why when the server did not check in time or
   cannot (`checking`, `noServer`, `serverMissing`...). Batch answers it per file.
5. **Files hidden from agents.** Settings → Tools → MCP Server → **Files hidden from agents**
   (`mcpRedactFiles` in `settings.json`): patterns as in 2, `!` keeping a file. The default hides
   `.env`, `.env.*` (not `.env.example`, `.sample`, `.template`), `*.pem`, `*.key`, `*.p12`,
   `*.pfx`, `*.jks`, `*.keystore`, `id_rsa*`, `id_dsa*`, `id_ecdsa*`, `id_ed25519*`, `.netrc`,
   `.pgpass`, `*credentials*`, `*.secret`, `*.secrets`. Over MCP (`ide/redact.rs`, before and after
   every tool call): a call whose path parameters (`path`, `paths`, `file`, `files`, `directory`,
   `sources`, `folder`, at any depth) name such a file is refused with
   `{"error": "redacted", "file"}`; text typed into a terminal that names one (`cat .env`) too; an
   answer's line of text of such a file (`text`, `preview` of an object whose `path` is the file, or
   a row `path:line:column text`) becomes `[redacted]`; `list_windows` marks its tab
   `"sensitive": true`. The window shows a notification naming the file each time. `set_settings`
   cannot change the list: only the user does. What a program prints in the terminal is not
   filtered: the setting keeps secrets out of an agent's ordinary reads, not out of reach of an
   agent allowed to run programs.
6. **Headless without the last tabs.** `<ide> --headless --no-restore` and `<ide> mcp --headless
   --no-restore` open the projects without the tabs and tool windows `workspace.json` remembers.
7. **What calls cost.** Every `tools/call` result carries `_meta.elapsedMs`, outside the payload a
   model reads (MCP's `_meta`); the server counts calls, errors, argument and answer bytes and time
   per tool (per command id for `dispatch_command`), and `mcp.session.stats` (`reset`) answers them
   (`zen_automation::stats`). It is a command like any other, so it is in the catalogue.

## Consequences

- Measured on this repository with rust-analyzer (docs/performance.md, "MCP against plain tools"):
  15 symbols 3 089 → 971 bytes; a file's errors 105 bytes; the project's errors 143 bytes; an edit
  with its new error 118 bytes in 0.3 s, against 9 s and a second call for `cargo check`.
- An agent can no longer read the default secret files through the IDE by accident; a project
  that needs an agent to read one says so with a `!` pattern.
- `_meta` adds about 25 bytes per answer on the wire; clients do not show it to the model.
- `--version` and compact test results were done before (ADR-0090).
