# ADR-0090: One-based places, one path per match, lean numbered reads and test results for agents

- **Status:** Proposed
- **Date:** 2026-10-22
- **Extends:** [ADR-0087](0087-agent-sized-mcp-answers.md) (agent-sized answers) and
  [ADR-0089](0089-mcp-answers-once-and-in-english.md) (answers once); part of
  developing Zen IDE through RustRonin

## Context

The first runs of agents through a headless RustRonin on this repository found three answers
that cost more than they should or disagreed with each other:

- **Test results.** `run.tests.get` answered the whole test tree: every package, test and
  subtest with its status, duration and console line ranges. An agent waiting for a run polls it:
  measured with RustRonin 0.2.4 on this repository's `zen-workbench` tests (604), 364 636 bytes per
  call (173 166 of payload, sent twice before ADR-0089), to learn the counts or which test failed.
- **Line numbers.** `search.text.find`, `search.text.results` and `search.text.preview` counted
  lines and columns from 0, while `navigate.*`, `problems.*` and `get_document_text` counted them
  from 1. An agent that read a match's lines with `get_document_text` was one line off.
- **Paths.** Every search match carried the file twice: the absolute `path` and the `relative`
  one. Measured on this repository, 60 matches of `TextMatch` were 18 580 bytes of payload, more
  than a third of it the absolute paths.
- **Numbered reads.** `get_document_text` with `withLineNumbers` put a tab after each number,
  which JSON escapes to two characters.

## Decision

1. **Test results by need.** `run.tests.get { runId, summary?, failedOnly?, test?, package? }`:
   - `summary`: `{ summary: { state, done, exitCode?, error?, counts, durationMs? } }`, a few
     hundred bytes, for polling;
   - `failedOnly`: the summary and `failed`, the tests that failed themselves (a parent that failed
     only through a failed subtest is left out), each `{ package, test, durationMs?, file?, line?,
     column?, message }`. The place is the first file position of the test's console output whose
     file exists (in the run's folder or the project; a panic inside the standard library names a
     file that does not), else the first one; `file` is relative to the project when it is in it.
     `message` is the output from that line on, without blank lines, at most 6 lines and 600
     characters. Every test runner's output works: the place comes from the console's links, which
     every run already detects;
   - `test` (with `package` when two packages have a test of that name): `{ output: { package,
     test, status, durationMs?, output } }`, that test's console lines, fetched on demand;
   - without them the tree, counts and `done` as before: the Run tool window reads it.
2. **Waiting for the end instead of polling.** `run.process.start` takes `waitMs` (at most
   600 000, ten minutes) and answers `{ runId, summary }` once the run has ended or the time is up.
   Dispatched directly (tests, the CLI) the command waits itself (`SharedOutput::wait_end`). An
   IDE answering an agent never waits on its UI thread: it starts the run, keeps the call
   (`IdeApp::waiting_runs`) and answers it from its loop when the run ends or the time is up, so
   its windows and other calls go on. The MCP server waits for the application up to its usual 30 s
   plus the `waitMs` a call asks for (`zen_automation::call_timeout`, at most ten minutes more).
3. **One numbering for places.** Every line and column an agent gets about a place in a file is
   one-based, columns in characters: `search.text.*` (matches, `search.text.preview`'s `line` and
   `first`), `navigate.*`, `problems.*`, `get_document_text` and failed tests. `navigate.*` and
   `problems.*` already were; `search.text.*` changes. Commands of the editor's own model keep its
   positions from 0 and say so in their schemas: `editor.selection.*`, `editor.caret.*`, sticky
   lines, code vision, `run.targets.list` and the VCS line ranges, which the UI and plug-ins drive
   with the editor's own coordinates; moving them is left for later (see Consequences).
4. **One path per match.** A search match is `{ path, line, column, length, text }`: `path` is
   relative to the project (with `/`) when the file is in it, absolute otherwise (a library's or a
   scratch file a scope adds). Every command that takes a path takes it as it is
   (`Workbench::resolve`, ADR-0087). `error` is left out when there is none.
5. **Lean numbered reads.** `withLineNumbers` gives `N|text` lines: the number, a bar and the
   line, the text plus a few bytes per line.
6. The server's `instructions` say so: one-based places, relative paths, `N|text`, and that tests
   are run with `waitMs` and read with `failedOnly`, never by polling the tree.

## Consequences

Measured on this repository through the headless RustRonin (0.2.4 before, this change after; the
"wire" is the whole JSON-RPC answer, ADR-0089's single text included):

| Answer | Before (wire / payload) | After (wire / payload) |
|---|---|---|
| `run.tests.get` of 605 `zen-workbench` tests, polled | 364 636 / 173 166 | `summary` 279 / 164 |
| the failed test with where and why | (the tree, then `run.output.get`) | `failedOnly` 880 / 735 |
| one test's output | (the console by ranges) | `test` 886 / 757 |
| `run.process.start` with the wait, to the end of the run | a start and polls | one call, 291 / 174 |
| `search.text.find` `TextMatch`, 60 matches | 38 412 / 18 580 | 10 855 / 9 866 |
| `get_document_text`, 66 numbered lines | 6 754 / 3 230 | 3 371 / 3 164 |

- Agents that read `relative` from search matches, or counted their lines from 0, must use `path`
  and one-based lines. The Find in Files window converts to the editor's coordinates itself.
- `withLineNumbers` lines change from `N\ttext` to `N|text`.
- The default `run.tests.get` answer is unchanged; `run.process.start` without `waitMs` too.
- A test whose output names no file has no `file`/`line`; its `message` is the start of its
  output.
- Not decided here: one-based positions for the editor's own commands, one path per usage in
  `navigate.usages.*` (a file's `path` and `relative`, and each usage's absolute `path`), and
  description cut at "e.g." in `list_commands`' one-line descriptions.
