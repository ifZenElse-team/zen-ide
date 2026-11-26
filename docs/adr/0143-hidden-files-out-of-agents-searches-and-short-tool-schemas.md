# ADR-0143: Hidden files out of agents' searches; short tool schemas

- **Status:** Proposed
- **Date:** 2026-10-25
- **Supersedes:** item 5 of [ADR-0139](0139-lean-mcp-answers-secrets-and-costs.md) for searches

## Context

ADR-0139 kept the files hidden from agents (`mcpRedactFiles`) out of reads, and showed a line of
such a file in a search's answer as `[redacted]`. The review of the published RustRonin 0.2.21
(2026-10-25) found that this still leaks the text: whether a search matches tells it, one character
at a time (`API_TOKEN=supers` matched, `supery` did not). It also measured `tools/list` at 66 KB,
about 16 000 tokens in every session, a third of it the full schema of `set_settings`.

## Decision

1. **An agent's search never looks into a hidden file.** When an agent runs a command
   (`zen_core::macros::by_agent`), the search scope (`search_scopes::resolve_in`, used by Find in
   Files, `search.text.find`/`start`, Search Everywhere's text, the Find tool window's searches,
   TODO and Find Usages' text matches) leaves out the files the window's `HiddenFromAgents` patterns
   name (the application gives every window the setting before an agent's call). Nothing of them is
   in the answer: no match, no name, no count of matches. `search.text.find` and
   `search.text.results` say `hiddenFiles: n`, the number of the project's files that are hidden,
   which does not depend on what is searched. Any other list in an answer of an agent's call (a
   language server's usages, the project's problems) loses its entries of hidden files, instead of
   showing them as `[redacted]`. The user's searches are unchanged.
2. **`set_settings` lists a short schema.** In `tools/list` its parameter is a free-form object
   with one line pointing to `get_settings` and `describe_command {"id": "set_settings"}`, which
   gives the full schema. The commands' own tools list their parameters without `"default": null`,
   number formats, a minimum of 0, and the `null` an optional parameter may be; `describe_command`
   still gives each command's schema as declared. `tools/list`: 65 968 → 42 148 bytes.
3. **The document in front counts as named.** An agent's call that works on the
   active document without naming a file (a command of the groups `editor.`, `navigate.`,
   `refactor.`, `problems.file.`, `problems.fix`, `spelling.`, `markdown.`, `localHistory.`,
   `file.document.`, `file.compare.`, `file.lineSeparator.`, `vcs.blame.`, `vcs.file.`,
   `vcs.history.`, `vcs.diff.`, `vcs.lineStatus.`, `http.request.`, `run.context.`, `run.targets.`,
   `search.text.occurrences` unless its schema requires a file of its own, whatever paths the call
   gives, since a command that does not take one ignores it; and `run_action` and `press_keys`,
   which act on what has the keyboard) is refused with `redacted` when a hidden file is in front, as
   a read by its path is. Naming another file (`file.document.open` with its path) brings it to the
   front and the commands work again.
4. **The real path decides** (review round 3, 2026-10-25). Whether a file is hidden is decided on
   its real path on disk (`hidden_files::is_hidden`): links to it or to a folder it is in followed,
   `.` and `..` resolved (for a file that does not exist yet, its nearest existing folder's real
   path with the rest), made relative to the project's real folder; a real path outside the project
   is matched whole. Patterns compare without case on macOS and Windows, whose disks open `.ENV` as
   `.env`, and with case on Linux. The same rule decides reads, opens, edits and writes, the
   searches' scope and their count, lists in answers, the document in front, and the words of
   terminal input (taken relative to each open project). A hard link has its own name and cannot be
   told from another file; an agent that may run programs in the terminal can still read any file.
   **Every path a command takes** (review of 0.2.24): besides the parameters known to hold paths
   (`path`, `paths`, `file`, `files`, `directory`, `sources`, `folder`, also for files that do not
   exist yet), every string of a call that names an existing file or folder, absolute or relative to
   an open project, is checked, whatever the parameter is called (`file.compare.files`' `left` and
   `right`, `file.compare.three`'s `middle`); only words (`text`, `oldText`, `newText`, `query`…)
   are not taken for paths. A test calls every command of the catalogue with the hidden file in each
   of its string parameters and finds its text in no answer.
5. **`.mcp.json`** starts `rustronin mcp --headless --no-restore .` (RustRonin 0.2.21 or newer).
6. **Agents neither read nor change hidden files** (review of 0.2.25 and the owner, 2026-10-25).
   - *Patches and diffs:* `vcs.patch.create` of local changes, a commit or a shelf leaves the hidden
     files out of the patch, its `files`, the clipboard and the file written, and says how many with
     `hiddenFiles`; the answer redactor also cuts the `diff --git` sections of hidden files out of
     any text it passes on.
   - *The IDE's own folder:* the data folder (the folder of the recent-projects file: Git history,
     shelves, Local History, caches, settings) holds copies of files, so a path whose real path is in
     it is hidden from agents whatever the patterns, but for the scratch files and buffers
     (`scratches`, `buffers`) and the language servers the IDE installed (`language-servers`:
     programs, whose entries `lsp.servers.list` would otherwise leave out); a data folder that holds an open project (a temporary folder in
     tests, a data folder set to a work folder) is not the IDE's alone and is left to the patterns.
   - *Changes:* a command an agent runs that would modify, move, delete, create, roll back,
     discard, stage, shelve, unshelve, apply a stash or patch over, or check out, merge, rebase,
     cherry-pick, revert or hard-reset over a hidden file is refused with `redacted`
     (`hidden_files::guard_change`, `guard_revision`, `guard_commit`), and so is one on a folder that
     holds one (deleting, renaming or moving it, a Git operation on it): never half done. A command
     on a set leaves them out and counts them: a stash (`git stash push` with the hidden files
     excluded, `hiddenFiles`), a changelist's commit or shelve, Discard or Stage of all changes,
     Replace in Files (`hiddenFiles`), line separators of a folder. A rename or an intention whose
     edit touches a hidden file is refused whole. The user's own commands are unchanged.
   - *Requests, copies and history* (from an independent review): an agent's HTTP request whose
     body reads a hidden file (`< file`, a multipart part, through a link) is refused before it is
     sent, and an exchange the user ran that read one shows an agent neither body; copying (copy
     into, copy and paste) is treated as moving: a hidden file or a folder holding one is never a
     source, and no copy lands where a pattern hides it; a folder's Local History leaves out the
     versions of hidden files (`hiddenFiles`) and its revert is refused when one would change; and a
     terminal word `<revision>:<file>` (`git show HEAD:.env`) counts as naming the file.
   - *Untracked files* (the owner, 2026-10-25): an agent's discard of everything
     (`vcs.changes.discard` with `all`) reverts tracked files only; it never deletes an untracked
     file or link, which an agent removes only by naming it (never a hidden one), and the answer
     says `untrackedKept: n`. The user's own Discard is unchanged.
   - *HTTP environments* (from a review): `http-client.private.env.json` is hidden by default;
     an agent's request takes no value from an environment file hidden from agents, and one that
     needs such a value is refused before anything is sent. Local History's `hiddenFiles` is the
     project's count, the same for any folder asked for, so it never tells which folders hold one.

## Consequences

- A search cannot tell whether a hidden file holds a text; an agent learns only that some files are
  hidden.
- About 6 000 fewer tokens in each agent session for the tool list.
- With a hidden file in front an agent first opens another file by its path.
