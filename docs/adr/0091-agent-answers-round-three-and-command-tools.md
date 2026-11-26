# ADR-0091: Whole descriptions, checked files, grouped usages, compact one-based run targets, long symbol waits, and the main commands as MCP tools

- **Status:** Proposed
- **Date:** 2026-10-22
- **Extends:** [ADR-0087](0087-agent-sized-mcp-answers.md) (agent-sized answers),
  [ADR-0089](0089-mcp-answers-once-and-in-english.md) (answers once) and
  [ADR-0090](0090-one-based-places-and-lean-test-results.md) (one-based places, one path per
  match); part of the work on developing Zen IDE through RustRonin

## Context

The dogfood runs through a headless RustRonin on this repository and the
review of RustRonin 0.2.5 found answers that were cut, ambiguous, repetitive or too short-lived:

- **Cut descriptions.** `list_commands { group }` gave each command's "first sentence" by cutting
  at the first `. `, so `problems.file.list` read "The problems the language servers and the
  inspections (e.g." — and every description was cut again at 120 characters. 288 of the
  catalogue's descriptions ended mid-sentence.
- **`[]` meant two things.** `problems.file.list` answered `[]` for a clean file and for a file no
  server had looked at yet (never opened, or edited a moment ago). An agent that checks its edit
  could not tell "no errors" from "not checked".
- **Usages repeated their file.** `navigate.usages.find` gave each file its absolute `path` and its
  `relative` one, and each usage the absolute path again: `Registry::dispatch`'s 1 436 usages in
  140 files were 530 KB. Show Usages rows each carried `path`, `relative`, `fileName` and `module`.
- **Run targets repeated their configuration.** `run.targets.list` gave every test its whole
  configuration (options, environment, working folder): 7.6 KB for the 11 targets of one file,
  360 KB for a file of 600 tests. Its `line` counted from 0, as did `run.context.get`'s, while
  everything else agents read is one-based (ADR-0090), so a failed test's line given to
  `run.context.get` landed one line off. A test file added while the IDE ran got no targets, or
  the targets of another file, until `cargo metadata` ran again.
- **Ten seconds were too short to load.** rust-analyzer needs 24–36 s to load this workspace; a
  symbol search with `waitMs: 60000` answered `pending` after 10 s (`MAX_WAIT`), so agents polled.
- **A missing server was hidden by the others.** In a Rust repository with JSON and shell servers
  but no rust-analyzer, Go to Symbol answered `noMatch`, and `problems.project.list` `[]`.
- **Every main command went through `dispatch_command`**, whose `params` is an untyped object: an
  agent had to call `describe_command` (or read the instructions) to know a command's parameters.

## Decision

1. **Whole first sentences.** A one-line description is the description's first sentence, whole:
   it ends at `.`, `?` or `!` followed by white space, not at an abbreviation's stop (`e.g.`,
   `i.e.`, `etc.`, `vs.`, `cf.`) and not inside brackets or backticks; there is no length cut.
2. **Checked or not.** `problems.file.list` answers the list only when the file's server has
   checked the file as it is now and found problems. Otherwise, and whenever there are none, it
   answers `{ problems, checked, reason?, message?, server?, install? }`: `checked: true` with no
   problems is a clean file; `checked: false` has `reason` `notChecked` (no diagnostics of the file
   have come), `checking` (the editor's text is newer than what the server checked: the document's
   version is not sent yet, or the server's `publishDiagnostics` `version` is older than its copy),
   `noServer` (no server or inspection checks such files) or a server's absence (ADR-0087). A file
   only an inspection checks is checked. The IDE's own views read both forms
   (`language::listed_problems`).
3. **Each path once in usages.** `navigate.usages.find`/`list` answer `files: [{ path, usages }]`,
   `path` relative to the project (absolute outside it) and each usage `{ start, end, line, column,
   preview?, highlight? }`. Show Usages answers its rows as `files: [{ path, module?, rows }]`, a
   row without its file (`comment` and `count` left out when false and 1). The popup reads its rows
   from the window's state (`show_usages::view`), not from the agents' JSON.
4. **Compact, one-based run targets.** `run.targets.list` answers `{ provider, line, kind, name,
   package? }` per target with a one-based `line`; `full: true` adds the `configuration`, which the
   gutter asks for (it converts the line to its own from 0). `run.context.get`'s `line` is one-based
   too. Rust's targets take a file Cargo would discover by its layout (`tests/x.rs`,
   `tests/x/main.rs`, `benches/`, `examples/`, `src/bin/`) as its own target when the project model
   does not list it yet.
5. **Long waits while loading.** `navigate.symbol.goTo` and `navigate.class.goTo` wait up to
   `waitMs`: at most 10 s (`MAX_WAIT`) as before, and up to **120 s** (`MAX_LOADING_WAIT`) while
   the answer is pending because a server is still loading (`scanning`, `starting`, `indexing`).
   Dispatched directly, the command waits itself (`Languages::search_waiting`). An IDE answering an
   agent never waits on its UI thread: it keeps the call (`IdeApp::waiting_lookups`), asks again
   with `waitMs: 0` from its loop every 100 ms and answers once the answer is no longer pending, the
   servers have loaded and 10 s have passed, or the time is up.
6. **A missing server beside the answers.** Go to Symbol and Go to Class answer `reason:
   serverMissing` with `server` and `install` beside the symbols the running servers found when a
   language of the project (or an open file) has no installed server
   (`Languages::missing_project_server`). `problems.project.list` answers `{ problems: [files],
   reason, server, install?, message }` in that case, and when no server runs and nothing is
   reported.
7. **The main commands are MCP tools of their own — yes, eight of them.** `editor.text.replace`,
   `search.text.find`, `navigate.symbol.goTo`, `navigate.usages.find`, `problems.file.list`,
   `refactor.rename.apply`, `run.process.start` and `run.tests.get` are also tools named after the
   command with underscores (`editor_text_replace`, ..., `run_tests_get`;
   `automation::COMMAND_TOOLS`). A tool's input schema is the command's parameter schema plus the
   optional `project`; its description is the command's; the IDE turns a call into the same
   `dispatch_command` (waiting runs and long symbol waits included), so the answers are identical.
   The server's `instructions` list them. `dispatch_command` stays the way to every other command.

   Why yes: these eight are most of an agent's calls (the dogfood logs: edits, searches, symbols,
   usages, problems, rename, runs and test results); as tools, their parameters are in the tool
   list the agent already has, so it neither calls `describe_command` nor guesses `params`, and a
   client that defers MCP tools (Claude Code loads a tool's schema when it is first needed) pays
   for each schema only once it is used. Why only eight: every tool costs every session its name
   and, in clients that do not defer tools, its schema — the eight add 15.6 KB to `tools/list`
   (26.4 → 41.9 KB); a tool per command (300+) would be the 360 KB catalogue again. Why their own
   names rather than new ones: the name maps back to the command id and its documentation.

## Consequences

Measured on this repository through a headless RustRonin: 0.2.5 (published) before, this change's
debug build after; bytes are the whole JSON-RPC answer.

| Answer | Before | After |
|---|---|---|
| `list_commands {group}` over all 22 groups | 91 465, 288 descriptions cut | 117 845, none cut |
| `problems.file.list`, a file never opened | 91 (`[]`) | 282 (`checked: false`, `notChecked`) |
| `problems.file.list`, an open clean file | 91 (`[]`) | 123 (`{"checked":true,"problems":[]}`) |
| `navigate.usages.find` `Registry::dispatch` (1 436 usages, 140 files) | 530 326 | 293 844 |
| Show Usages, 100 rows in 14 files (payload) | 45 482 | 21 445 |
| `run.targets.list`, `automation.rs` (11–13 targets) | 7 577 | 2 173 |
| `run.targets.list`, a file of 600 tests added while running | 360 005 (wrong target: `run`) | 83 025 (its own target) |
| `navigate.symbol.goTo` `waitMs: 60000`/`120000` while rust-analyzer loads | `pending` after 10.0 s, no symbols | symbols after 13.3 s |
| `tools/list` | 26 374 (36 tools) | 41 948 (44 tools) |
| `initialize` instructions | 2 672 characters | 2 914 characters |

- Agents that read `files[].relative`, a usage's `path`, Show Usages' `rows`, `run.targets.list`'s
  `configuration` or its lines from 0, or `problems.file.list`'s `[]` must adapt: the list of a
  clean file is `{"problems": [], "checked": true}`.
- `list_commands {group}` is larger (whole sentences); `{query}` stays the cheap way to look.
- A symbol search can now take two minutes when an agent asks for it while a server loads; it
  never blocks the IDE's windows.
- Not decided here: one-based positions for the editor's own commands (`editor.selection.*`,
  `editor.caret.*`, sticky lines, code vision, VCS line ranges); absolute paths in symbol and class
  answers; a `waitMs` for `problems.file.list` that has a file not open checked.
