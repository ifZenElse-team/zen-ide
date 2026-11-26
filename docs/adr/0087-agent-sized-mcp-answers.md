# ADR-0087: Agent-sized MCP answers: one-call edits, ranged reads, a catalogue on demand, no silent empty answers

- **Status:** Proposed
- **Date:** 2026-10-21
- **Extends:** [ADR-0033](0033-ai-access-to-every-api.md) (every capability reachable by AI),
  [ADR-0084](0084-headless-ide.md) and [ADR-0085](0085-headless-ide-per-agent-session.md) (the
  headless IDE agents use); the requirement: MCP for AI agents cheaper than working without the IDE

## Context

An agent working through a Zen IDE's MCP server paid more than with its own tools. Measured through
`--headless` on a Go module of 800-line files (and, by a sister session, on a real task):

- a small edit took five calls (open, read the whole file, set the selection, insert, save) and
  about 17 KB of answers, against one call of about 400 tokens without the IDE;
- `get_document_text` could only return the whole file (16 KB for 800 lines);
- `list_commands` returned every command with its full description and JSON schema: 552 commands,
  364 KB, about 90 000 tokens, before the agent did anything;
- searches answered "nothing" without saying why: `search.text.find` with an absolute `directory`
  (it took only relative ones) answered no matches; `navigate.symbol.goTo` right after
  `open_project` answered `{"symbols": [], "pending": false}` while the project's files were still
  being listed and its language server had not even started.

Every IDE has the same MCP surface (it is the platform's), so the fix is the platform's.

## Decision

1. **`editor.text.replace`** (zen-workbench) replaces exact text in one call:
   `{path, oldText, newText, expectedCount?, edits?, save?}`. It searches the file as the editor has
   it (unsaved changes included; a file no editor has open is opened), requires `oldText` to occur
   exactly `expectedCount` times (1 by default) and replaces every occurrence as one undo step;
   `edits` makes several replacements of one file, found in the text as it was before the call, all
   or none; `save` saves with the on-save actions as `file.document.save` does. It answers
   `{changed, line, replaced?, saved?}`, never the text. Errors carry a stable code and facts:
   `not_found`, `not_unique` with `count` and `expected`, `overlap`; an edit of a batch is named by
   `edit` (its index).
2. **Coded errors reach agents as JSON.** `CommandError` (zen-core) gets `data`, facts next to
   `code`. When a command fails with both, `dispatch_command` answers the error text
   `{"error": <code>, ...data, "message": <words>}`; every other error stays words, as before.
3. **Ranged reads.** `get_document_text` takes `startLine`/`endLine` or `offset`/`limit` (lines,
   one-based, inclusive) and `withLineNumbers`, and answers `startLine`, `endLine` and
   `totalLines` with a range. A file larger than **32 KB** (about 8 000 tokens; an 800-line Go file
   is 16–25 KB) without a range is the error `{"error": "too_large", "lines", "bytes", "message"}`
   pointing to ranges. A smaller file without a range answers `{text, unsaved}` exactly as before.
4. **The catalogue on demand.** `list_commands {}` answers every command id by group
   (`{"groups": {"editor": ["text.replace", ...]}}`, about 8 KB); `{group}` or `{query}` (words that
   must all occur in the id, title or description) answers those commands' id, title and the first
   sentence of the description, at most 120 characters; `describe_command {id}` answers one
   command's description and parameter schema. The old answer, every command with its schema, is
   kept only behind `list_commands {full: true}`, for compatibility. The server's `instructions`
   name the command groups with their sizes and the 18 commands agents need most (edit, read,
   search, symbols, usages, problems, rename, imports, format, run, tests) with their parameters,
   so that `list_commands` is rarely needed.
5. **No silent empty answers.** Search and navigation answers say why they have nothing:
   - `navigate.symbol.goTo`, `navigate.class.goTo` and `navigate.usages.find`/`list` add `reason`:
     `scanning` (the project's files are being listed, so which servers it needs is not known yet),
     `starting`, `indexing` (a server reports work under way), `searching`, while `pending`;
     `noMatch` or `noServer` once done with nothing; and `progress` while indexing;
   - **indexing comes from the protocol, for every server:** the language-server layer keeps each
     server's work-done progress (`$/progress` `begin`/`report`/`end`; the client already
     announces `window.workDoneProgress`) as `{server, title, message?, percentage?}`. A symbol
     answer that came while its server reported work is pending (`indexing`) and is asked again
     once the work has ended, so an empty answer given while loading is never kept;
   - `search.text.find`/`results` add `reason` (`searching`, `noMatch`) to an empty answer;
   - **paths:** `search.text.*`'s `directory` is relative to the project or absolute (spelled from
     the project, through links); a folder outside the project or missing from it is an error
     that says so. `file.document.open`, `problems.file.list`, the lookups at a place (usages,
     declaration, rename) and `get_document_text` take a path relative to the project too
     (`Workbench::resolve`).

## Consequences

- Measured through the headless IDE on the same module: a small edit is
  one call with a 40-byte answer (was five calls, 17 KB); 40 lines of an 800-line file are 1 KB
  (was the whole 16 KB file); the catalogue is 8 KB (was 364 KB); one schema 1–2 KB.
- An agent that called `list_commands {}` for schemas gets ids only and must ask with `group`,
  `query`, `describe_command` or `full: true`. `get_document_text` without a range fails for files
  over 32 KB. Both are visible changes for agents, which is why this is a decision.
- `reason` and `progress` are new optional fields; `pending` keeps its meaning. A server that
  reports long work (e.g. a build) keeps symbol answers `pending` until it ends.
- Not decided here (later items of the same requirement): compact rows for search answers, diagnostics
  filters, usages by name, problems in an edit's answer, hiding secret files, `--no-restore`,
  compact test results, timings.
