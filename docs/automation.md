# Automation: AI agents in the IDE

> **Last updated:** 2026-10-22 · **Decisions:** [ADR-0033](adr/0033-ai-access-to-every-api.md),
> [ADR-0042](adr/0042-zen-cli-output-and-exit-codes.md), [ADR-0084](adr/0084-headless-ide.md),
> [ADR-0085](adr/0085-headless-ide-per-agent-session.md), [ADR-0087](adr/0087-agent-sized-mcp-answers.md),
> [ADR-0089](adr/0089-mcp-answers-once-and-in-english.md)

Every Zen IDE can be driven by AI agents through the Model Context Protocol (MCP). The server runs
inside the IDE, listens on `127.0.0.1` only and refuses requests from web pages (non-local
`Origin`). While it runs, the status bar shows **MCP**.

## Turning it on

- **Settings → Tools → MCP Server** (⌘,): "Enable MCP Server" and the port (GoGarden 63530,
  RustRonin 63531, WebWave 63532, JvmJizo 63533, SqlScroll 63534, DevopsDaruma 63535). Applied at once, saved in `settings.json` in the IDE's data folder.
- Or for one run: `ZEN_AUTOMATION=1` (default port; any value that is not a port above 1 means it
  too) or `ZEN_AUTOMATION=<port>`; `0` or an empty value leaves it off. The variable never
  changes `settings.json`: the server runs when the setting or the variable asks for it, the
  variable's port wins for that run, and saving any setting keeps the user's own MCP choice.

The running server's URL is written to `automation.json` in the data folder
(`~/Library/Application Support/space.ifzenelse.gogarden/` on macOS).

## Connecting an agent

The recommended way: a headless IDE for the project, started with the agent's session and ended
with it ([`<ide> mcp --headless`](#mcp---headless-a-headless-ide-per-agent-session)). Run this in the
project's folder; Claude Code saves it in the project's `.mcp.json`:

```sh
claude mcp add -s project gogarden -- /Applications/GoGarden.app/Contents/MacOS/gogarden mcp --headless .
```

Nothing has to be kept open, and a windowed GoGarden is never used or touched.

Over HTTP, while the IDE runs:

```sh
claude mcp add --transport http gogarden http://127.0.0.1:63530/mcp
```

Over stdio, for agents that start MCP servers as programs; it starts the IDE with the server on
when it is not running:

```sh
claude mcp add gogarden -- /Applications/GoGarden.app/Contents/MacOS/gogarden mcp
```

The Settings page shows the three commands with the right paths, each with a Copy button.

## Tools

| Tool | What it does |
|---|---|
| `list_windows` | Open project windows, their documents (with unsaved changes) and the active one |
| `list_commands` | The command catalogue on demand (see [Working cheaply](#working-cheaply-edits-reads-and-the-catalogue)): `{}` every command id by group, `{group}` or `{query}` those commands' id, title and one-line description, `{full: true}` every command with its description and parameter schema (the old answer, about 360 KB); every description in a schema is one line, without line breaks or backslashes |
| `describe_command` | `{id}`: one command's id, title, description and parameter schema |
| `dispatch_command` | Runs any command in a project window (`project` names the window when several are open); an id that is a keymap action instead (`view.settings.open`) runs as `run_action` does |
| `run_action`, `get_window_state`, `press_keys` | The window itself: run any keymap action or command as its shortcut does, including the `view.*` actions that open tool windows, popups and dialogs; what the window shows; keys pressed in its next frame (see [Window actions](#window-actions)) |
| `open_project`, `focus_project`, `close_project` | Project windows |
| `exit_ide` | Exits the IDE as ⌘Q does (a headless IDE exits at once, see [Headless IDE](#headless-ide)). With `confirmed: true` it exits at once, never blocked by the Confirm Exit dialog; without it the dialog asks (popup `confirmExit`, ⏎ exits, ⎋ cancels) unless `confirmExit` is off. Answers `exiting`, `asking` and `running`: the work exiting stops (`kind` `dependencyUpdate`, `git`, `task`, `run` or `debug`, and `title`). The keymap action `view.ide.exit` (⌘Q) takes the same `confirmed` |
| `list_recent_projects` | Recent projects |
| `get_document_text` | A file's text as the editor has it, including unsaved changes; a range of lines with `startLine`/`endLine` or `offset`/`limit`, numbered with `withLineNumbers`; a file over 32 KB only by ranges |
| `terminal_open`, `terminal_write`, `terminal_read` | Terminal tabs in a project folder: shortcuts for `terminal.session.new`, `.send` and `.read` on a tab's focused session (see [Terminal](#terminal)) |
| `list_plugins`, `install_plugin`, `uninstall_plugin`, `set_plugin_enabled` | Plug-ins: installed, the catalog, install or update, remove, switch on and off; the `plugins.*` commands do more (see [Plug-ins](#plug-ins)). Their fields are unchanged; `list_plugins` adds `installingIds` (every id queued) and the catalog's versions `notes`, `install_plugin` adds `queued` and `installingIds`. An agent's install or update of a plug-in that brings programs running on the computer (a database driver, ADR-0174) or terminal agent adapters (`terminalAgents`), or that installs one with it, is refused with code `userOnly`, and so is switching such a plug-in on while it is off: the user installs and switches it on from Settings → Plugins |
| `list_themes`, `select_theme`, `follow_system_theme`, `import_theme` | Themes: list them with the one in use and whether it follows the system, use one in every window (remembered), follow the system's light or dark appearance (Sync with OS), import a Zen, JetBrains or VS Code theme file (light ones build on Zen Light; faint text is made readable); the same as the `ui.theme.*` commands, without a project window (docs/themes.md) |
| `get_tree_selection`, `select_in_tree` | The selection of the Project tool window, which its actions (Delete, Copy) act on; `select_in_tree` also takes files and folders of External Libraries (a crate's or a module's sources), unfolded under their library |
| `get_update_status`, `update_ide` | A new version on offer; install it and restart |
| `get_settings`, `set_settings` | Every setting of the Settings dialog; `set_settings` merges objects key by key (see [Settings](#settings)) |
| `find_action` | Find Action (⇧⌘A): the keymap's actions and every other registered command matching a name, an abbreviation or a synonym, with shortcuts and the command; `run_action` runs one by its `action` (see [Window actions](#window-actions)) |
| `get_keymap`, `set_keymap` | The keymap in use and its id: every action with its keystrokes, abbreviations, synonyms and conflicts; change an action's keystrokes in the keymap in use (ADR-0038), also of a command `find_action` lists without one, as ⌥⏎ in Find Action does (ADR-0102) |
| `list_keymaps`, `select_keymap`, `import_keymap` | Keymaps: list them with the one in use, use one in every window (remembered), import a JetBrains keymap `.xml` with a report of what was not carried over; the same as the `ui.keymap.*` commands (ADR-0051, docs/jetbrains-compatibility.md) |

A new feature is not done until agents can reach it (ADR-0033): add its command first, then the UI.

**Paths through symbolic links.** An open project is known by its real path (`list_windows`
reports it), even when it was opened through a link. A path an agent gives that reaches the
project's files through a link (`/tmp/app/main.go` for the project `/private/tmp/app`) is spelled
from the project's real path before it is used: the `project` argument, the paths of
`select_in_tree` and `get_document_text`, and the `path`, `paths`, `file`, `files`, `directory`,
`sources` and `folder` parameters of `dispatch_command` and `run_action`, at any depth
(`zen_vfs::spelled_from`). Links inside the project keep their names, as the tree shows them.

## What an answer looks like

A successful `tools/call` answers the tool's result as compact JSON, **once**, in one `text`
content block: `{"content": [{"type": "text", "text": "{\"windows\":[...]}"}], "isError": false}`.
No `structuredContent` is sent, because no tool declares an `outputSchema`; MCP 2025-06-18 requires
`structuredContent` only of a tool that does, and asks such a tool to put the same JSON in a text
block too ([spec, Tools → Structured Content](https://modelcontextprotocol.io/specification/2025-06-18/server/tools)).
`zen_automation::Tool::output_schema` declares one; such a tool answers both forms. A failed call
answers `isError: true` with the words, or the coded-error JSON (see below), as text.

Which form an agent gets, measured 2026-10-21 with Claude Code 2.1.306: when both are present it
hands the model `structuredContent` and drops the text; otherwise the text. Until GoGarden 0.38.29
and RustRonin 0.2.4 the IDE sent both, so each answer crossed the wire twice for nothing. Sizes of
typical answers on a small Go module, before → after: `problems.file.list` 1 563 → 862 bytes,
`navigate.symbol.goTo` 14 978 → 7 975, `list_windows` 468 → 277
([ADR-0089](adr/0089-mcp-answers-once-and-in-english.md)).

## Trust and safe mode

A project the person has not trusted is in safe mode ([ADR-0205](adr/0205-trusted-projects.md),
[ADR-0248](adr/0248-safe-mode.md)): files open and can be edited, and no language server, go.mod
check, `cargo metadata`, project formatter, run or debug session starts. Commands that would run
something answer with code `notTrusted` (a refused command answers `{"error": code, "message"}`, with
any other fields of the refusal). `open_project` opens in safe mode without asking, and a
headless IDE says at its start which projects are in safe mode.

| Command or tool | What it does |
|---|---|
| `project.trust.state` `{ path? }` | `{ path, trusted }` |
| `project.trust.grant` | The person's: refused for agents with `agentNotAllowed` |
| `project.trust.revoke` `{ path? }` | Stops trusting a project; open to agents |
| `project.trustedLocations.get` | `{ locations: [{ path, coversHome }] }`, Trusted Locations in their order |
| `project.trustedLocations.set` `{ locations }` | Replaces the list; agents may remove and reorder, and a new folder is refused with `agentNotAllowed` |
| `view.project.trust` | The banner's **Trust project…**, which shows the dialog to the person; refused for agents |

`get_window_state` also answers `headless` and `session`, an overview of the session in one call: the
project, its branch with its upstream and how far ahead or behind, how many files changed, each
language server's state and whether the text index is ready; a headless IDE answers it as a
windowed one does. `get_window_state` answers `safeMode`, and the popup `trustProject` while **Trust and Open Project**
waits for the person. `press_keys` is refused while it is up: only the person answers it.

## Language for agents

Everything the IDE says to agents is in English by default, whatever language the system and the
IDE's interface speak: command errors and their `message`, the reasons of empty answers, Find
Action's names, refusals. Tool descriptions, the server's `instructions` and command titles and
descriptions are English in any case. **Settings → Tools → MCP Server → Language for agents**
(`set_settings` with `mcpLanguage`: `en` or `ru`) chooses another language; error codes and fields
(`not_unique`, `count`, `reason`) are the same in every language, and the interface keeps its own.
The IDE answers each call inside `zen_i18n::with_language(mcpLanguage)`, which changes the language
of that thread only while the call is answered
([ADR-0089](adr/0089-mcp-answers-once-and-in-english.md)).

## Working cheaply: edits, reads and the catalogue

An agent working through the IDE must pay less than with its own tools
([ADR-0087](adr/0087-agent-sized-mcp-answers.md)); this is the platform's, so every IDE has it.

**Instructions.** The server's `instructions` (sent with `initialize`) name the command groups with
their sizes and the commands agents need most (edit, read, search, symbols, usages, problems,
rename, imports, format, run, tests) with their parameters (`automation::MAIN_COMMANDS`), so that
`list_commands` is rarely needed.

**The catalogue on demand.** `list_commands {}` answers `{ groups: { editor: ["text.replace", ...],
... } }`: every command id by group (the group is the id's first part), about 8 KB.
`list_commands { group }` and `list_commands { query }` (words that must all occur in the id, title
or description) answer `{ commands: [{ id, title, description }] }` with the first sentence of the
description, whole: it ends at a full stop followed by a space, never at an abbreviation's (`e.g.`,
`i.e.`, `etc.`) and never inside brackets or backticks ([ADR-0091](adr/0091-agent-answers-round-three-and-command-tools.md)). `describe_command { id }` answers one command's `id`, `title`,
`description` and `params` (its JSON schema, without `$schema` and `title`), 1–3 KB.
`list_commands { full: true }` answers every command with its schema, as earlier versions did.

**One-call edits.** `editor.text.replace { path, oldText, newText, regex?, expectedCount?, edits?,
save? }` finds `oldText` in the file as the editor has it (unsaved changes included; a file no
editor has open is opened), requires it to occur exactly `expectedCount` times (1 by default) and
replaces every occurrence with `newText` as one undo step. With `regex: true`, `oldText` is a
regular expression (`^` and `$` match at every line's start and end, case matters) and `newText`
fills in its groups as Replace in Files does (`$1`, `${name}`, `$0`; `$$` is a `$`).
`edits: [{ oldText, newText, expectedCount?, regex? }]` makes several replacements of the file, each
found in the text as it was before the call: all of them or none. `save: true` saves afterwards
with the on-save actions (`editor.onSave.*`: optimize imports, reformat, then the On Save options
of the next section), as `file.document.save`
does. An agent's edit is saved unless it passes `save: false`, so that its shell, git and scripts
read what it wrote; that default save leaves the on-save actions out, so a reformat never changes
lines the agent did not touch (an explicit `save: true` runs them); the same holds for `editor.text.write`, `editor.text.batch` and the open files
`refactor.rename.apply` changes. A file that changed on disk while the editor holds unsaved edits
is not overwritten: the save fails and the call says so. In a CRLF or CR file, `\n` in `oldText` and `newText` stands for the file's line
separator (ADR-0097). The answer is `{ changed, line, replaced?, saved? }`: whether the text changed, the line of
the first replacement, how many places when more than one, and whether it was saved.

**What a save does to whitespace** (Settings → Editor → General → On Save, ADR-0106). After the
actions on save and before the file is written, a save removes the trailing spaces and tabs of the
lines changed since the last save (`removeTrailingSpacesOn: "all"`: of every line), except on caret
lines (`keepTrailingSpacesOnCaretLine`), Markdown hard breaks and string literals that go on to the
next line; optionally removes the trailing blank lines (`removeTrailingBlankLines`) and adds a final
line break in the file's own separator (`ensureLineBreakAtEnd`). It is one undo step
(`editor.edit.undo` brings the spaces back) and never touches read-only files. The project's
`.editorconfig` `trim_trailing_whitespace` and `insert_final_newline` win over the settings for
the files they match.
`editor.onSave.options { path? }` answers what applies to a file: `{ path, removeTrailingSpaces
("off", "modified" or "all"), keepCaretLines, removeTrailingBlankLines, ensureLineBreak,
trimTrailingWhitespace?, insertFinalNewline? }`, the last two being what its `.editorconfig` says.
An agent that wants its own trailing spaces kept places a caret on that line or turns
`removeTrailingSpaces` off with `set_settings`.

`editor.text.write { path, text, save? }` gives a file its whole text: a file of the project that
does not exist is created, with the folders on the way, and opened (`created: true`); an existing
one gets the text in the editor as one undo step. `editor.text.batch { files: [{ path, oldText?,
newText?, regex?, expectedCount?, edits?, text? }], save? }` does both for several files in one
call: every file is checked first and every file changes, or none does; the answer is `{ files:
[{ path, changed, line, replaced?, created?, saved? }] }`, each `path` relative to the project.
`file.fs.newFile`, `newDirectory`, `rename`, `delete`, `copyInto` and `project.file.paste` take
paths relative to the project too (`.` is the project folder).

Why these exist ([ADR-0092](adr/0092-places-by-line-and-editing-through-the-ide.md)): measured on
this repository, the agent of round three edited with Python scripts because a new file could not
be given its text (`oldText` may not be empty), a change of four files took four calls and a change
of a pattern took one exact edit per place. This round's 32 file edits took 38 calls of
`editor.text.replace`; with `editor.text.batch` they are 21. Errors are JSON, with a code and facts:

| Error | When | Facts |
|---|---|---|
| `not_found` | `oldText` is not in the file | `count: 0`, `expected`, `edit` (a batch's index), `file` (`editor.text.batch`'s index) |
| `not_unique` | it occurs another number of times than `expectedCount` | `count`, `expected`, `edit`, `file` |
| `overlap` | two edits of a batch change the same text, or a batch names a file twice | `file` |
| `invalid_regex` | `regex` is set and `oldText` is not a valid regular expression | `edit`, `file` |

Any command error with a code and facts reaches agents the same way: `dispatch_command` answers
the error text `{"error": <code>, ...facts, "message": <words>}` (`CommandError::data`, zen-core);
other errors stay words.

**Ranged reads.** `get_document_text { path, startLine?, endLine?, offset?, limit?,
withLineNumbers? }`: lines are one-based and the range inclusive; `endLine` defaults to the last
line, `offset`/`limit` is the first line and how many. A ranged answer is `{ text, unsaved,
startLine, endLine, totalLines }`; with `withLineNumbers` each line is `<number>|<text>`
([ADR-0090](adr/0090-one-based-places-and-lean-test-results.md)), a few bytes more than the text per
line. Without
a range a file up to 32 KB answers `{ text, unsaved }` as before; a larger one is the error
`{"error": "too_large", "lines", "bytes", "message"}`, which points to ranges.

**Paths.** `file.document.open`, `editor.text.*`, `problems.file.list`, the lookups at a
place (`navigate.usages.find`, `navigate.declaration.goTo`, `refactor.rename.*`) and
`get_document_text` take a path relative to the project too (`Workbench::resolve`).

**Places by line and column.** The lookups at a place (`navigate.usages.find`, `navigate.usages.show`,
`navigate.declaration.goTo` and every command that takes `LookupParams`, `editor.highlights.get`,
`refactor.rename.prepare`/`preview`/`apply`) take `line` and `column`, one-based as every answer
gives them (`column` 1 when absent; a column past the line's end is its end), as well as a character
`offset`, which wins when both are given. A line the file does not have is an error that says how
many lines it has (ADR-0092).
`search.text.*`'s `directory` is relative to the project or absolute; a folder outside the project
or missing from it is an error that says so, never "no matches".

**One numbering for places** ([ADR-0090](adr/0090-one-based-places-and-lean-test-results.md)).
Every line and column an agent gets about a place in a file is one-based, columns in characters:
`search.text.find`/`results`/`preview`, `navigate.*` (symbols, classes, usages, Show Usages,
recent locations, history), `problems.*`, `get_document_text`, the failed tests of
`run.tests.get`, and `run.targets.list`'s and `run.context.get`'s `line` (ADR-0091). A search match names its file once, in `path`: relative to the project (with `/`)
when the file is in it, absolute otherwise; every command that takes a path takes it as it is.
For an agent, the targets of `navigate.declaration.goTo` (and the other `navigate.*.goTo` lookups and
`navigate.lookup.poll`), the files and changes of `refactor.rename.preview` and `refactor.rename.apply`,
and the paths `file.fs.delete` answers are relative the same way; the window keeps its own paths.
Commands of the editor's own model keep its positions from 0 and say so in their schemas
(`editor.selection.*`, `editor.caret.*`, sticky lines, code vision, the VCS line ranges); character
`offset`s are offsets, not lines. Usages name each file once too: `navigate.usages.find`/`list`
answer `files: [{ path, usages: [{ start, end, line, column, preview?, highlight? }] }]`, and Show
Usages `files: [{ path, module?, rows }]` (ADR-0091). The `path` of each symbol of
`navigate.symbol.goTo` and each type of `navigate.class.goTo` is relative to the project too, with
`/`, and absolute outside it (the SDK, libraries): 605 bytes became 242 for one symbol of this
repository (ADR-0092).

**Checked or not.** `problems.file.list` answers the list when the file's server has checked the
file as it is now and found problems; otherwise, and whenever there are none, `{ problems, checked,
reason?, message? }`: `checked: true` with no problems is a clean file; `checked: false` says why in
`reason`: `notChecked` (no diagnostics of the file have come: it is not open in its server, so
`file.document.open` it), `checking` (it changed and the server's new check has not come: the
problems listed are of the text before; ask again), `noServer` (nothing checks such files), or a
server that is not there (`serverMissing`, `serverNotRunning`, `starting`, with `server`,
`install?` and `message`). The server's `publishDiagnostics` `version` tells an old check from a
current one (`Languages::analysis`); a server that publishes only diagnostics that changed, as
rust-analyzer does, leaves a file checked two seconds after its change when it reports no work
under way (`QUIET_CHECK`).

**A search the server dropped is asked again.** When a language server answers Find Usages with
`content modified` (-32801) or `server cancelled` (-32802), as rust-analyzer does while it takes in
a change, `navigate.usages.find` asks it again, up to three times within its `waitMs`, instead of
answering that error (`Languages::ask_again`).

**Longer symbol waits while a server loads.** `navigate.symbol.goTo` and `navigate.class.goTo` wait
up to `waitMs`: at most 10 s (`MAX_WAIT`), and up to 120 s (`MAX_LOADING_WAIT`) while the answer is
pending because a server is still loading the project (`scanning`, `starting`, `indexing`), as
rust-analyzer does for half a minute on this repository. An IDE answering an agent never waits on
its UI thread for that: it keeps the call (`IdeApp::waiting_lookups`) and asks again from its loop
until the answer has come, the servers have loaded or the time is up. A server that ends its work
is asked the query again (what it answered meanwhile may be incomplete); the 10 s of a search that
is no longer loading count from the moment the loading ended (`SearchWait`), so that answer is
waited for. Before, a search that had loaded for 13 s answered `searching` at once with the server
idle, and the agent had to ask again: on this repository the first Go to Symbol took two calls and
38 s through RustRonin 0.2.6, and takes one call and 12 s now (ADR-0092). A caller that gives `waitMs` gets it
whole, loading or not, up to 120 s: the 10 s after a load hold only for the IDE's own searches
([ADR-0283](adr/0283-a-callers-symbol-wait-is-honoured.md)).

**Plug-in runs and agents.** A configuration of kind `plugin` (a plug-in's run target,
ADR-0286) starts for an agent only as the person allows it per plug-in (`plugins.runAccess.get`,
`plugins.runAccess.set { plugin, access }`, the person's alone): `none`, the default, answers
`agentNotAllowed`; `ask` answers `confirmationNeeded` with `data.confirmation`, posts a notification
whose **Allow once** runs `plugins.runAccess.allow { token }` (the person's alone), and the same
`run.process.start` with `confirmation` then starts it once; `full` starts it. Agents always read
the targets (`run.targets.list`) and the configurations. A run whose command its plug-in declares
person-only (`"personOnly"`, ADR-0291: Terraform's `apply`, `destroy`, `import`, `state`...) answers
`userOnly` to an agent whatever the setting; a person's start answers `personConfirmation` and the
window asks the person with the whole command line, and while it asks, or while such a run runs,
`press_keys` and window actions are refused.

**Project indexes** ([ADR-0293](adr/0293-gitops-project-index.md)). A plug-in that declares an
`"index"` gets the project's matching files in the background and answers an index of nodes,
edges, references and problems (the GitOps plug-in: Flux and Argo CD objects).
- `plugins.index.get { plugin?, kinds?, path?, references?, waitMs? }` answers
  `{ indexes: [{ plugin, kinds, nodes, edges, problems, references, files, truncated, indexing }], indexing }`.
  - It is narrowed by plug-in, node kind and file.
  - `waitMs` waits for an index being built.
  - For an agent, what is in files hidden from agents is left out.
- `plugins.index.refresh { plugin? }` builds the index again now.
- A tool window section with `"rows": "index"` (the GitOps window) answers
  `plugins.toolWindow.rows` with the index's nodes (`columns`: name, kind, namespace, path;
  `state`: `error` or `warning` when the node has problems). It needs no trust, and it runs
  nothing.
  - **Cluster state.** When the section declares a cluster state (the GitOps window,
    [ADR-0299](adr/0299-cluster-state-beside-a-project-index.md)), each node row also gets
    `cluster: { state, columns }`.
  - The answer gets `cluster: { from, values, columns, errors, listing?, retryAfterMs? }`. The state
    is read with the Kubernetes window's context, in trusted projects only, with `refresh` at most
    every 2 minutes after a failure unless `force`.
  - `plugins.toolWindow.act` runs the section's row actions on those rows (GitOps: `events`, Show
    Events).
- An index's problems come with a file's other problems in `problems.file.list`, with the plug-in's
  id as their source, as long as the file's text is still the indexed one.
- An index's `completions` (names offered after a prefix such as `.Values.` in the files they are
  for) come in `editor.completion.start`; nodes with `inGraph: false` (a chart's values keys) are in
  `plugins.index.get` but not in the window's rows
  ([ADR-0297](adr/0297-index-completions-hidden-nodes-and-output-documents.md)).
- `navigate.declaration.goTo` on an index reference goes to its target. For a folder, the window
  shows it in the Project tool window.
- `navigate.usages.find` on a reference, or on a declaration's name, lists the references to it on
  a Find tab.

Both are read-only: nothing runs and nothing is downloaded.

**Plug-in tool windows** ([ADR-0289](adr/0289-plugin-tool-windows.md)).
`plugins.toolWindow.list` answers the windows plug-ins declare (sections, columns, actions with
their `kind`); `plugins.toolWindow.rows { plugin, window, section, refresh?, force?, waitMs? }` a section's
rows `{ id, columns, state? }`, with `error` and `retryAfterMs` after a failed listing, `busy`
while an action on the section runs (then nothing is listed again) and `listing` while a listing
runs past `waitMs` (20 s by default; the window passes 0 and is never held up); `plugins.toolWindow.act {
plugin, window, section, action, rows, waitMs?, confirmation? }` runs an action on rows by their
ids, which must be ids the platform listed in this session: values are never taken from the call.
Listing is open to agents; `stream` (a run, `runId`), `terminal` (a Terminal session: arbitrary
execution in, say, a container) and `change` follow the plug-in's run access above; `destroy`
always answers `confirmationNeeded` first, and only the person's `plugins.toolWindow.allow { token
}` (from the notification) lets the same call with `confirmation` run once. In a project nobody
trusted every one of them answers `notTrusted`, for the person too.
An action with `inputs` ([ADR-0295](adr/0295-values-a-person-types-into-tool-window-actions.md):
`count` 0–1000, `port` 1024–65535, `forward` `LOCAL:REMOTE`) takes `inputs: { <id>: "<digits>" }`
from the person only, checked before the run; an agent's `inputs` answer `agentNotAllowed`, and an
agent's call without them answers `confirmationNeeded` with a token while a notification's **Enter
Values…** (`plugins.toolWindow.enterValues { token }`, the person's) opens the window's dialog; the
person's `plugins.toolWindow.allow { token, inputs }` keeps the values for the same call with
`confirmation`, once. `plugins.toolWindow.list` answers each action's `inputs` (kind and bounds).
A window with header choices ([ADR-0290](adr/0290-tool-window-choices-and-confirmations.md), the
Kubernetes window's context and namespace) lists them in `plugins.toolWindow.list` (`choices`)
and answers them with every section's rows: `choices: [{ id, title, value?, valid, options: [{ id,
label }], note?, source? }]` (`source`: `[{ path, exists }]`, the files of the variable the choice
names, such as `KUBECONFIG`, as the programs get it, ADR-0294). Only the person changes them (`plugins.toolWindow.choose { plugin, window,
choice, value }` answers agents `agentNotAllowed`); every listing and action passes their values,
an action is refused with `choiceNeeded` while one is not valid, and an action marked `confirms`
answers `confirmationNeeded` with the question naming them (the context, the rows) every time, for
agents whatever their run access, as a `destroy` does.

**The login shell's environment** ([ADR-0294](adr/0294-the-login-shells-environment.md)).
`ide.environment.get` answers `{ enabled, state, shell?, names, error? }`:
- `enabled` is the setting `environment.loginShell`;
- `state` is `notRead`, `reading`, `ready` or `failed`;
- `names` are the variables that runs, language servers, plug-in tool windows and targets and the
  terminal get from the person's login shell, read once at start, and never their values;
- `error` is why the shell gave none.

**Schemas.** `schemas.catalog.list` answers the schemas the plug-ins ship (`schema` as
`<plugin>/<id>`, `name`, `server`, `files`, `uri`), the person's `mappings` and `downloads`;
`schemas.mappings.set { mappings: [{ schema, files }] }` replaces the mappings (agents may) and
restarts the servers with schemas; `schemas.downloads.set { allowed }` is the person's alone
(`agentNotAllowed`). Downloads are `http(s)` only, never in a project nobody trusted, announced in
a notification before they start, and kept for the session (ADR-0288).

**Compact run targets.** `run.targets.list { path, full? }` answers each target as `{ provider,
line, kind, name, package? }` with a one-based `line`; `run.process.start { context: { path, line }
}` runs it. `full: true` adds the `configuration` of each, as the gutter needs it. A file added
while the IDE runs (`tests/x.rs`, `benches/`, `examples/`, `src/bin/`) is the target Cargo would
discover there, before `cargo metadata` runs again. A plug-in's targets may also be on a folder
(`run.targets.list { path: <folder> }`, line 1) and use the values chosen in its tool window
(`choiceNeeded` when there are none); a target whose output is a diff ends with each changed
object opened in the diff viewer, and agents read the diff itself with `run.output.get`
([ADR-0296](adr/0296-a-manifest-against-the-cluster.md)). A target whose output is a document
(`helm template`, `kustomize build`) opens its standard output read-only in an editor tab when it
exits with 0; agents read it with `run.output.get`
([ADR-0297](adr/0297-index-completions-hidden-nodes-and-output-documents.md)).

**Main commands as tools.** The most used commands are MCP tools of their own as well, named after
the command with underscores (`editor_text_replace`, `editor_text_write`, `editor_text_batch`,
`search_text_find`, `navigate_symbol_goTo`,
`navigate_usages_find`, `problems_file_list`, `refactor_rename_apply`, `run_process_start`,
`run_tests_get`; `automation::COMMAND_TOOLS`): their arguments are the command's parameters and
the optional `project`, their answer is `dispatch_command`'s, and the server's `instructions` list
them (ADR-0091).

**No silent empty answers.** A search or lookup that has nothing says why, in `reason`:

| `reason` | Meaning | `pending` |
|---|---|---|
| `scanning` | the project's files are still being listed, so which servers it needs is not known yet | true |
| `starting` | a language server is starting | true |
| `indexing` | a server reports work under way (`$/progress`, e.g. gopls loading packages); `progress` says which: `{ server, title, message?, percentage? }` | true |
| `searching` | a server or the text search has not answered yet | true |
| `noMatch` | the search is done and nothing matches | false |
| `noServer` | no running server of the project answers this | false |
| `serverMissing` | the server that would answer is not installed; `server` names it and `install` says how to install it | false |
| `serverNotRunning` | the server is installed but does not run (it failed, was stopped or turned off, or nothing it serves is open); `server` names it | false |
| `notTrusted` | the project is not trusted, so its language server does not start; only the person trusts it (Trust Project, `project.trust.grant`) | false |

**A missing server says so.** When the language server a file or the project needs is not there,
the language commands answer why instead of an empty list or a bare "not running"
(`Languages::absence_for`, `project_absence`), with `server` (its id, e.g. `rust-analyzer`), a
`message` and, when it is missing, `install`: `{ command: "lsp.server.install", params: { id },
runs }` (`runs` is the command line it runs, e.g. `rustup component add rust-analyzer`) when the IDE
can install it, else `{ hint }`:

- `problems.file.list` with no problems answers `{ problems: [], checked: false, reason, server,
  install?, message }` (with problems, the list as before);
- `problems.project.list` answers `{ problems: [files], reason, server, install?, message }` when a
  server the project needs is missing (its files' problems are unknown), or when none runs and
  nothing is reported;
- `navigate.symbol.goTo` and `navigate.class.goTo` answer `reason` `serverMissing` (or
  `serverNotRunning`) with `server` and `install` when no server of the project could answer, and
  `serverMissing` beside the symbols the other servers found when a language of the project has no
  server (a Rust repository with JSON and shell servers but no rust-analyzer). A
  missing server is named only when the search found nothing, and only a server that finds
  declarations across the project (`workspaceSymbols`, false for the Dockerfile, YAML, TOML and
  JSON servers): on this repository a Rust query was answered 'Dockerfile Language Server is not
  installed' (ADR-0169);
- `editor.structure.get` without symbols adds `reason`, `server` and `install` (its words are in
  `message`, which was `reason` before);
- the lookups at a place (`navigate.usages.find`, `navigate.showUsages`, `navigate.declaration.goTo`,
  `navigate.implementation.goTo`, `refactor.rename.*`) fail with the coded error
  `{"error": "serverMissing" | "serverNotRunning" | "starting", "server", "install"?, "message"}`;
- `language.servers.status` and `language.servers.sync` list the missing servers the project or an
  open file needs, `{ state: "missing", install?, message }` (`install` only in the status).

The headless IDE writes to its standard error, when it starts, which servers each project needs
that it found (with their programs) and which are missing, with the command that installs them.
`run.targets.list` and `run.context.get` do not need a server: Rust's `#[test]`,
`#[tokio::test]`, `#[bench]` and `fn main` come from the source, as the gutter's run icons.
`a_missing_server_says_so_instead_of_empty_answers` and
`symbol_search_names_the_missing_server_when_none_can_answer` (`language/servers/tests.rs`) check
the answers.

`navigate.symbol.goTo` and `navigate.class.goTo` answer `reason` with an empty list or while
pending, `navigate.usages.find`/`list` while pending and with `noMatch`, `search.text.find` and
`search.text.results` with an empty list. The work-done progress is kept for every server by the
language-server layer (`Languages::work_progress`, `busy`): `begin` adds a work, `report`
updates it, `end` removes it. A symbol answer that came while its server reported work is
`pending` (`indexing`) and the query is asked again once the work ends, so an empty answer given
while loading is never kept.

`an_agent_edits_reads_and_lists_in_a_few_bytes_over_mcp` (`ide/headless/tests.rs`) checks the edit
(one call, under 300 bytes), the `not_found` error, a 40-line range of an 800-line file (under
3 KB), `too_large`, the catalogue (under 20 KB) and one schema (under 3 KB) over HTTP;
`an_answer_given_while_the_server_loads_is_pending_and_asked_again_after` (`language/symbols.rs`)
the `indexing` answer and the second request.

## Lean answers, hidden files and what calls cost

[ADR-0139](adr/0139-lean-mcp-answers-secrets-and-costs.md) P1–P2:

- **`compact: true`** turns a list of places into rows of text. `navigate.symbol.goTo` and
  `navigate.class.goTo`: `rows` of `path:line:column kind name` (`… type Params in ListFonts` when
  the name does not say what it belongs to); `editor.structure.get`: `line:column kind name detail`,
  two spaces of indent per level; `navigate.usages.find`: `path:line:column text`. The object list
  (`symbols`, `classes`, `files`) is then empty.
- **Problem filters.** `problems.file.list` and `problems.project.list` take `severity` (`error`:
  errors only; `warning`: errors and warnings; `weakWarning`), `sources` (`["rust-analyzer",
  "clippy"]`, ignoring case), `limit` and `compact` (rows `line:column severity message [source
  code]`, `path:` before them in the project's list); the project's list also `paths` and `exclude`,
  patterns of project-relative paths (`crates/*/src/*.rs`, `*` crossing folders), file names
  (`*.go`) or folders (`vendor`). A filtered answer is `{problems, checked, reason?, truncated?}`
  for a file and `{problems, total, truncated?, reason?}` for the project.
- **Usages by name.** `navigate.usages.find` takes `symbol` (`Store.Get`, as Go to Symbol takes
  it; the best match whose name is the last part) or `line` with `name` instead of a column. With
  `waitMs` the usages come in the answer. `includeTextMatches: true` adds `textMatches`, rows of the
  whole-word occurrences of the name the server does not count: comments, strings, other languages,
  other symbols of the same name.
- **Edit reports.** `editor.text.replace`, `write` and `batch` take `checkProblems: true`: the
  answer adds `newProblems`, the errors and warnings the edit brought, once the file's server has
  checked the new text (`checkMs`, 3 000 by default, at most 10 000), e.g. `{"changed": true, "line":
  38, "newProblems": ["38:28 error no field `resett` on type `StatsParams` [rust-analyzer E0559]"]}`;
  `[]` when it brought none, and `unchecked` (`checking`, `noServer`…) when the server did not check
  it in time.
- **Files hidden from agents.** `mcpRedactFiles` (Settings → Tools → MCP Server → Files hidden from
  agents) lists patterns as above, `!` keeping a file; by default environment files (not their
  `.example`, `.sample`, `.template`), keys, certificates, key stores, SSH keys, `.netrc`,
  `.pgpass`, `*credentials*` and the HTTP Client's `http-client.private.env.json`. A call whose `path`, `paths`, `file`, `files`, `directory`,
  `sources` or `folder` names such a file is refused with `{"error": "redacted", "file", "message"}`,
  as is terminal input that names one (`cat .env`). An agent's searches never look into such files
  ([ADR-0143](adr/0143-hidden-files-out-of-agents-searches-and-short-tool-schemas.md)): the scope of
  `search.text.find`, `search.text.start`, Search Everywhere's text, the Find tool window's
  searches, TODO and the text matches of Find Usages leaves them out, and `search.text.find` and
  `search.text.results` say only how many files are hidden (`hiddenFiles`, the same whatever is
  searched), so a right and a wrong guess of a secret answer the same. Any other list in an answer
  (a language server's usages, the project's problems) leaves out the entries of such files too;
  `list_windows` marks a hidden file's tab `"sensitive": true`. With a hidden file in front, a
  command that works on the active document without a path (`editor.*`, `navigate.*`,
  `problems.file.list`, `localHistory.*`…) and `run_action`/`press_keys` are refused with `redacted`
  as a read by path is; opening another file by its path brings that one to the front. The window tells the user which file was kept from the agent. `set_settings`
  refuses to change the list. Output of programs in the terminal is not filtered.
- **Short tool schemas.** `tools/list` was 66 KB, a third of it `set_settings`' schema; that tool
  now takes a free-form object (`describe_command {"id": "set_settings"}` gives every key), and the
  commands' own tools list their parameters without `"default": null`, number formats, a minimum of
  0 or the `null` an optional parameter may be: 42 KB (ADR-0143).
- **Costs.** Every tool result carries `_meta: {"elapsedMs": n}` beside its content.
  `mcp.session.stats` answers `{total, tools}` since the server started (`reset: true` starts
  again): per tool, and per command id for `dispatch_command`, `calls`, `errors`, `bytesIn` (the
  arguments), `bytesOut` (the answers' text) and `elapsedMs`.

Tests: `language/agent_tests.rs` (compact rows, filters, usages by name and text matches, edit
reports with a scripted server), `ide/redact.rs`, `secrets_are_kept_from_agents_and_the_window_says_which`
and `no_restore_opens_the_project_without_its_last_tabs` (`ide/headless/tests.rs`, over HTTP), and
`zen_automation::stats`. The measurements on this repository are in
[performance.md](performance.md#mcp-against-plain-tools-2026-10-24).

## Language servers

The language servers are commands of the project window too
([ADR-0055](adr/0055-language-servers.md)); the guide's [Language servers](guide/17-language-servers.md)
page says what the user sees:

| Command | Parameters | Result |
|---|---|---|
| `lsp.servers.list` | `{}` | `{ servers }`: each server the IDE knows with `id`, `name`, `state` (`running`, `starting`, `failed`, `stopped`, `missing`, `notStarted`, `disabled`), `version?`, `path?`, `args`, `files` (open files it serves), `queued` (messages waiting to be written to it: a thread of its own writes them, so a slow server never holds up the window), `languages`, `extensions`, `names` (the files it serves), `enabled`, `message?` (why it failed, how an installation goes, how to install it), `installable`, `installer?` (`{ tool, download }`: the missing `go`, `npm`, `cargo` or `rustup` and where to get it), `installing`, `needed` (the project has or the user opened files it serves), `dismissed`, `busy` (it runs and reports work in progress, or has not finished loading the project: its answers may be incomplete), `progress` (`[{ title, message?, percentage? }]`, oldest first), `readyAfterMs` (how long after its start it was first not busy) and `javaNeeded?` (the Java version a server run with Java needs and no JDK found has: Eclipse JDT LS needs 25; `toolchain.jdk.download` gets one) |
| `lsp.server.install` | `{ id }` | `{ started: true }`; installs in the background with `go install`, `npm install -g`, `cargo install` or `rustup component add`, or downloads, checks and unpacks the server's archive and its files (Eclipse JDT LS with java-debug), only when called. `lsp.servers.list` follows it |
| `lsp.server.restart` | `{ id }` | `{}`; stops the server and starts it again, with its current program and arguments, when a file it serves is open |
| `lsp.server.stop` | `{ id }` | `{}`; the server stays stopped until restarted |
| `lsp.server.log` | `{ id, since? }` | `{ lines, next }`: the server's standard error, the JSON-RPC trace and its installations, the last 2 000 lines; pass `next` as `since` to get only newer lines |
| `lsp.server.dismiss`, `lsp.server.undismiss` | `{ id }` | `{}`; "Don't ask again" about installing the server, saved in `language-servers.json` |
| `lsp.server.configure` | `{ id, path?, args?, enabled? }` | `{}`; the program (a path or a name looked up), the arguments and whether the server may start, saved in `language-servers.json`; the server restarts with them in this window. An absent field is kept; `null` (or `""` for `path`) returns to the default |
| `language.servers.status` | `{}` | The started servers by id (`gopls`), each `{ state, reason? }` with `state` `starting`, `running`, `stopped` or `failed`, and the servers the project or an open file needs that are not installed, `{ state: "missing", install, message }` with `install` `{ command: "lsp.server.install", params: { id }, runs }` (or `{ hint }` when the IDE cannot install it) |
| `todo.patterns.list` | `{}` | `{ patterns, words, filters }`: the TODO patterns (regular expressions, `\btodo\b.*` and `\bfixme\b.*` by default, set with `set_settings` and `todo`), the filters' names and the words they start with (`TODO`, `FIXME`), which the completion offers in a comment (items of kind `text`) when what was typed starts one, case-insensitively, and is not the word itself; `editor.completion.accept` puts such a word in at every caret whose typed letters start it, leaving the other carets as they are |
| `editor.completion.list` | `{ since? }` | The completion in progress: `{ open, pending, prefix, items, error?, revision, unchanged }`, `items` the matching ones, best first (`index`, `label`, `kind`, `detail?`, `documentation?`, `matches`). The items are matched and sorted again only when what was typed, the server's answer or the error changes, and `revision` changes with them; pass the last `revision` as `since` to get `unchanged: true` and no items while they are the same (the editor does, every frame the popup is open) |

## Terminal

The Terminal's tabs and split panes are commands of the project window (`zen_workbench::terminal`);
the guide's [Terminal](guide/07-terminal.md) page says what the user sees. Every pane is a session
with its own shell and PTY and a process-wide `id`; the sessions live in the window's session, so
they keep running while the tool window is hidden. `id` and `tab` are optional where marked: the
focused session of the tab shown, and the tab shown.

| Command | Parameters | Result |
|---|---|---|
| `terminal.session.new` | `{ cwd? }` | `{ id, tab }`; the user's login shell in a new tab, shown and focused; `cwd` is absolute or relative to the project (the project's folder by default) |
| `terminal.session.split` | `{ id?, direction }` | `{ id, tab }`; a new shell in the pane's folder, `right` of it or `down` below it, focused |
| `terminal.session.list` | `{}` | `{ tabs, active }`: each tab with `index`, `title`, `name` (the user's, or `null`), `active`, `focused`, `layout` (`{ session }` or `{ direction, ratio, first, second }`) and `sessions` (`id`, `title`, `cwd`, `exited`, `focused`, `elsewhere: true` for a session in an older IDE's terminal host this IDE cannot show, and `rerun`, the last command offered again in a terminal that came back after a restart, `terminal`, the session's `ZEN_TERMINAL_ID`, and `resumed`, the agent whose conversation it resumed) |
| `terminal.session.send` | `{ id?, text }` | `{}`; types `text` (at most 64 KiB, else an error) as the keyboard would, `\n` presses Enter |
| `terminal.session.read` | `{ id? }` | `{ id, text, truncated, title, exited }`: the visible screen, at most 64 KiB from its end (`truncated` when cut); the scrollback is not returned |
| `terminal.session.close` | `{ id? }` | `{}`; ends the shell and removes its pane; a tab left without panes closes (⌘W in the terminal) |
| `terminal.session.focus` | `{ id }` | `{}`; gives the session the keyboard and shows its tab |
| `terminal.session.focusNext`, `terminal.session.focusPrevious` | `{}` | `{ id }`; the next or previous pane of the tab shown (⌥⇥, ⌥⇧⇥) |
| `terminal.tab.select`, `terminal.tab.close` | `{ tab? }` | `{}` |
| `terminal.tab.rename` | `{ tab?, name? }` | `{}`; `null` or an empty name gives the tab back its own title |
| `terminal.selection.get` | `{ id? }` | `{ id, text, truncated }`: the selection as ⌘C copies it (no blanks at line ends, wrapped lines joined, real line breaks kept), at most 64 KiB from its end; `text` is `null` when nothing is selected |
| `terminal.selection.copy` | `{ id?, plain? }` | as `get`, and puts the text on the clipboard (Copy, Copy as Plain Text: the terminal copies plain text either way); an error when nothing is selected |
| `terminal.selection.selectAll` | `{ id? }` | as `get`, after selecting the screen and the whole scrollback history |
| `terminal.lines.copy` | `{ id?, from, to }` | `{ id, text, truncated, lines }`; copies lines `from` to `to` (from 1, the oldest of the history; `lines` in all) as Copy gives them; an error outside 1 to `lines` |
| `terminal.session.paste` | `{ id? }` | `{}`; pastes the clipboard as ⌘V does, bracketed when the program asks; refused to agents with `userOnly` |
| `terminal.session.rerun` | `{ id? }` | `{}`; runs again the last command of a terminal that came back after a restart (its `rerun` in `terminal.session.list`); the person's choice: refused to agents with `userOnly` |
| `terminal.session.dismissRerun` | `{ id? }` | `{}`; hides that offer, nothing runs |
| `terminal.agent.list` | `{}` | `{ adapters, reports }`: the agent resume adapters of plug-ins and Settings (`id`, `name`, `programs`, `resume`, `sessionField`, `docs`, `source`: `settings`, `bundledPlugin` or `plugin`, whose resume command is only offered to the person, `hook`: the session-start hook command that reports the agent's conversation id) and the folder the hooks write to (`ZEN_TERMINAL_AGENTS`) |
| `terminal.agent.report` | `{ id? \| terminal?, agent, session }` | `{}`; keeps the conversation id `session` of the adapter `agent` for a terminal (its session `id`, or its `ZEN_TERMINAL_ID` as `terminal`), as the hook's file does; only for an adapter that exists (agents cannot add one: `set_settings` refuses `terminalAgents`); after a restart the tab runs that adapter's resume command with it |
| `terminal.link.list` | `{ id? }` | `{ id, links }`: the links on the session's visible screen, top to bottom, each with `text` (as printed), `kind` (`url` or `file`), `target` (the URL or the absolute path), `line` and `column` when printed, and `row`, `startColumn`, `endRow`, `endColumn` (one-based); paths that name nothing are left out |
| `terminal.link.open` | `{ target, action?, id?, dryRun? }` | `{ kind, action, target, line?, column?, copied?, program?, args?, done }`; what ⌘-click offers: `openInBrowser` (a URL's default; refused to agents with the code `userOnly`) or `copyLink`; `openInEditor` (a file's default, at its line and column), `revealInProject` (a folder's default), `openInFinder` or `copyPath`. Relative paths start from the session's folder, then the project |
| `java.jshell.open`, `kotlin.repl.open` | `{ module?, classpath?, waitMs? }` | `{ session?, module, classpath, command }`; JShell of the project JDK, or the installed `kotlinc`'s REPL, in a new Terminal tab, with the module's classpath made as for an Application run in the background (`session` is absent until it starts; `waitMs` waits, at most 10 minutes); `kotlin.repl.open` is refused when no `kotlinc` is installed, which the IDE never downloads (ADR-0230) |
| `java.jshell.restart`, `java.jshell.stop`, `kotlin.repl.restart`, `kotlin.repl.stop` | `{}` | restart answers as open, with the newest console's module and classpath built again; stop `{}`, refused when none runs |

Links are found by `zen_terminal::links`, one (wrapped) line at a time, at most 4096 characters
and 64 links a line, with no regular expressions and no file system: URLs with a known scheme
(`http`, `https`, `file`, `ftp`, `ftps`, `ws`, `wss`, `ssh`, `git`), and paths that start with `/`,
`./`, `../` or `~/`, hold a folder or are a file name with an extension, with `:line[:column]`,
`(line[,column])` or Python's `", line N` after them. Whether a path names something is decided
apart (`links::resolve`), in the session's folder and then the project, dropping Git's `a/` and
`b/` prefixes when only that finds it. The window shows a link only while ⌘ is held; the action
⌘-click takes without asking is the `terminalLinks` setting (`{ url, file }`, `null` asks).

A tab's title is its name, else the title its focused shell set (OSC 0 or 2) while it is at most
24 characters, else "Local", "Local (2)"... by the tab's number (the smallest one free). A
session's own `title` is the shell's title, else the name of the folder it started in. A pane
whose shell ends is removed. `workspace.json` keeps each project's tabs
as `terminal`: their names, splits with their ratios, and the folders the sessions started in (the
shell's later `cd` is not followed: the emulator does not pass the shell's OSC 7 reports on); they
start again, with new shells, when the Terminal shows or any `terminal.*` command runs (also
`terminal.session.list`, so agents see them while the Terminal stays hidden), and a saved folder
that is not absolute, is gone or cannot be read starts in the project's folder. The keymap has a
`terminal` context for ⌘T, ⌘W and ⌥⇥, which win over the window's bindings while a terminal pane
has the keyboard; of the window's bindings only ⌘ chords and `view.toolWindow.terminal` (⌥F12)
apply there, so ⌃C, ⌃D, ⌃R, the function keys and the rest go to the shell. JetBrains keymaps map
`Terminal.NewTab`, `Terminal.SplitVertically`, `Terminal.SplitHorizontally`,
`Terminal.NextSplitter` and `Terminal.PrevSplitter` onto these bindings. Closing a pane or a tab,
or dropping the window's session, hangs up the shell's PTY and waits for it, so the shell and its
jobs end and leave no zombie.

**Consent.** `terminal.session.send` and `terminal_write` type into the user's shell, so whatever an
agent sends runs as the user. They are reachable only as the other commands are: through the MCP
server, which is off until the user turns it on (Settings → Tools → MCP Server, or `ZEN_AUTOMATION`
for one run), listens on `127.0.0.1` only and shows **MCP** in the status bar while it runs.
Plug-ins cannot dispatch commands.

## New Project

Projects are created in the background, also with no project open
([ADR-0237](adr/0237-new-project-wizard-and-project-generators.md)). A job answers `state` `running`,
`ready` or `failed`; ask again, or give `waitMs` (at most 60 000).

| Command | Parameters | Answer |
|---|---|---|
| `project.new.generators` | `{}` | `{ generators, defaultDirectory, suggested, networkAllowed }`: the project types in the wizard's order, each `{ id, name, icon, section, plugin?, description?, network, settings?, formDependsOn?, tool? }` (`settings`: `{ key, label, help?, default }`), a free location, and whether generators may use the network |
| `project.new.form` | `{ generator, retry?, waitMs?, values?, module? }` | `{ state, form?, error? }`: the type's fields, a `jdk` field with the JDKs of `toolchain.jdk.list` by home, with `form.notice` when they come from saved data; `retry` loads them again |
| `project.new.check` | `{ location }` | `{ path, name, problem?, notEmpty }`: `problem` is why Create is disabled |
| `project.new.preview` | `{ generator, name, values?, waitMs? }` | `{ state, files?, error? }`, each file `{ path, text?, size }`; nothing is written. The wizard has no such page: this is for agents |
| `project.new.create` | `{ generator, location, values?, git?, reuse?, waitMs? }` | `{ job, state, path?, kept?, trusted, error? }`; refused with `invalidLocation`, `folderNotEmpty` (without `reuse`) or `unknownGenerator`; `failed` when a required `jdk` field has no JDK. A `jdk` field's JDK becomes the project's. It does not open the project: call `open_project` with `path` |
| `project.new.status` | `{ job, waitMs? }` | as `project.new.create` |
| `project.new.cancel` | `{ job }` | `{ cancelled }`: what was written is removed, a running program is stopped |
| `project.newLists.get` | `{ generator, list }` | `{ rows }`: a list a type keeps (a field's `manage`) |
| `project.newLists.set` | `{ generator, list, rows }` | Person only (`agentNotAllowed`). `{ rows }`: load the form again to see them |
| `project.fromVcs.defaults` | `{}` | `{ urls, directory }` |
| `project.fromVcs.check` | `{ url, directory? }` | `{ suggested, problem? }`: `problem` is why Clone is disabled; no network |
| `project.fromVcs.clone` | `{ url, directory, depth?, waitMs? }` | as `project.new.create`, with `progress`; follow it with `project.new.status`, stop it with `project.new.cancel`. Never trusted |
| `project.fromSources.detect` | `{ path }` | `{ root, build, buildFile? }`: `build` is `maven`, `gradle` or `plain`; call `open_project` with `root` |
| `project.module.options` | `{}` | `{ generators, location, build?, values }` for the open project |
| `project.module.new` | `{ generator, name, location?, values?, reuse?, waitMs? }` | as `project.new.create`, with `registered`, the build file, and `reload`, the command to run next; refused with `buildFileUnsaved` while that file has unsaved changes |
| `project.module.fromSources` | `{ path }` | `{ path, registered, changed, reload }`; refused with `noBuild` or `buildFileUnsaved` |

A generator with a `tool` (Maven Archetype) is refused for agents with `notTrusted`: its template runs
code, so only a person starts it (ADR-0244). `output` in the job's answer has the tool's last lines.

`run_action` with `view.newProject.open { generator? }` or `view.newProject.empty` (no longer in the menu) opens the wizard
(`view.newProject.close` closes it); `get_window_state` lists `newProject` in `popups` and gives its
state in `newProject`. A failure leaves nothing behind. A project an agent creates, also through the
wizard, is not trusted (ADR-0205): the person trusts it. `view.module.new { generator? }` opens the
wizard for a module, `view.project.fromVcs` Clone Repository (`view.project.fromVcs.close` closes
it; `clone` in `popups` and in the state), and `view.project.fromSources { path }` and
`view.module.fromSources { path }` do without the chooser, which only a person sees.

`set_settings` changes what generators may do ([ADR-0240](adr/0240-generators-with-a-service.md)):
`generatorsNetwork` (`false` keeps every generator off the network) and `generatorSettings`, the
values of their settings by `key`, e.g. `{"generatorSettings": {"spring-boot.initializr.serviceUrl":
"https://start.example.com"}}`; `null` gives a key its default back. A change drops the forms and
previews loaded so far.

## Help

`run_action` with `view.whatsNew.open` opens the running version's release notes in an editor tab
and answers `{ path }`, refused while none are kept. `view.keyboardShortcuts.open` writes the active
keymap's reference as a PDF in the data folder, opens it and answers `{ path }`;
`view.keyboardShortcuts.export { path, format? }` writes it to `path` as `pdf` or `html` (the file
extension when `format` is omitted); an agent's `path` must be a new file inside the project, not
hidden from agents. `view.tipOfTheDay.open { index? }` opens the dialog (popup
`tipOfTheDay`) and answers `{ index, count, action, text }`. `view.productivity.open` opens My
Productivity (popup `myProductivity`) and answers `{ features, used }`, with `used` the rows counted
so far (`action`, `title`, `group`, `times`, `lastUsed`, seconds since 1970);
`view.productivity.reset` clears them. What an agent runs is not counted. The settings
`whatsNewOnUpdate`, `showTipsOnStartup` and `tipIndex` go through the settings tool.

While no project is open, `run_action` and `dispatch_command` reach the welcome window:
`view.welcome.open { section? }` (`projects`, `customize`, `plugins`, `learn`; also from a project,
where it brings the welcome window forward) answers `{ section }`, `get_window_state` answers
`{ welcome: { section }, popups }`, `view.settings.open { page? }` and
`view.ideSettings.import { path? }` open their dialogs there, and `ui.theme.*`, `ui.keymap.*`,
`settings.ide.*` and `plugins.*` work as they do in a project. `view.guide.open { chapter? }` opens
a chapter of the guide (the guide's page name, such as `database`, or `keyboard-shortcuts`) in the
browser and answers `{ url }`; an unknown chapter is refused with the list.

## Plug-ins

Settings → Plugins is made of commands too (ADR-0031, ADR-0036); the guide's
[Plug-ins](guide/10-plugins.md) page says what the user sees. The catalog is fetched only by
`plugins.catalog.list`, the commands that install or update, and `plugins.updates.check`; while it
loads, `catalog` is `loading`, so call again. Installs and updates run one after another in the
background: `installing` lists the ids still to do, and `plugins.installed.list` shows the result.

| Command | Parameters | Result |
|---|---|---|
| `plugins.catalog.list` | `{ query? }` | `{ catalog, plugins, installing, restartRequired, checkFailed? }`: `catalog` is `notFetched`, `loading`, `ready` or `unavailable`; `plugins`, sorted by name and matching every word of `query` in the name, id, description or file types, each with `id`, `name`, `description`, `installed?` (the version here), `latest?`, `update?` (a newer version for the installed one), `bundled`, `enabled`, `installing`, `extensions`, `names` and `languageServers` (it declares some) |
| `plugins.installed.list` | `{ query? }` | The same for the installed plug-ins, bundled ones included; `update` is known only once the catalog is fetched, and this command never fetches it |
| `plugins.plugin.details` | `{ id }` | The plug-in as above and `notes`: `[{ version, date, notes }]`, the release notes (Markdown, cut to 64 KiB) of the newest version and, when older, of the installed one |
| `plugins.plugin.install` | `{ id }` | `{ queued, installing }`; the newest version of a catalog plug-in, after the plug-ins it `requires` that are not installed (Nuxt brings Vue) |
| `plugins.plugin.update` | `{ id }` | `{ queued, installing }`; refused when nothing newer is published |
| `plugins.updates.check` | `{}` | Fetches the catalog again and returns `plugins.installed.list`; if that fails, the catalog fetched before stays in use and `checkFailed` says why |
| `plugins.updates.all` | `{}` | `{ queued, installing }`: every installed plug-in with an update |
| `plugins.plugin.enable`, `plugins.plugin.disable` | `{ id }` | `{ id, restartRequired }`; an agent switching on a plug-in that brings database driver programs or terminal agent adapters gets `userOnly` |
| `plugins.plugin.uninstall` | `{ id }` | `{ id, restartRequired }`; refused for a bundled plug-in, whose update only it removes |

`restartRequired` names the plug-ins whose language servers change only when the IDE restarts;
highlighting, folding, the outline and completion change at once.

### Plug-ins of a project

A plug-in may declare the projects it supports, by files at their root or packages in their
`package.json`, with run configurations to add and a setup step
([ADR-0129](adr/0129-project-plugins-npm-run-and-nuxt.md), [plugins.md](plugins.md)). The window
offers the catalog's plug-ins for the open project in a notification (`view.plugin.install { id }`,
`view.plugin.ignoreForProjects { id }`) and applies the installed ones with these commands:

| Command | Parameters | Result |
|---|---|---|
| `plugins.project.list` | `{}` | `{ plugins }`: the installed plug-ins that are on and support the open project, each `{ id, name, runConfigurations, setup? }`, `runConfigurations` the names it adds and `setup` `{ plugin, pluginName, name, creates, needed }` (Nuxt: `nuxi prepare`, `.nuxt`, whether `.nuxt` is missing) |
| `plugins.project.apply` | `{}` | `{ added, offers }`: the configurations added to the project (`{ plugin, id, name }`), once per project and plug-in, and the setup steps to offer (as `setup` above), each once per project. The window calls it when the project opens and the plug-ins change |
| `plugins.project.setup` | `{ plugin?, waitMs? }` | `{ runId, summary? }`, as `run.process.start`: runs the plug-in's setup step (the first one needed without `plugin`) in the Run tool window |

### Built-in plug-ins

The plug-ins built into every Zen IDE as native modules (Go, Rust...; ADR-0071) are switched on and
off for the next start; Settings → Plugins → Built-in shows them. A module that is off has no
commands at all: `toolchain.*` exists only while Go is on.

| Command | Parameters | Result |
|---|---|---|
| `plugins.modules.list` | `{ query? }` | `{ product, modules, restartRequired }`: each module with `id` (`go`, `rust`, `database`), `name`, `description`, `enabled` (on from the next start), `active` (this run has it), `onByDefault` (in this product) and `restartRequired` (`enabled` differs from `active`); `restartRequired` at the top names the modules waiting for a restart |
| `plugins.module.enable`, `plugins.module.disable` | `{ id }` | The module as above; the choice is saved in `modules.json` in the IDE's data folder and applies after a restart |
| `go.inspections.run`, `go.inspections.status` | `{}` | `{ staticcheck, running, install }`: `run` checks the packages of the open Go files with `go vet` and `staticcheck` now, in the background; `running` lists the folders still being checked. The findings are in `problems.file.list` with `source` `go vet` or `staticcheck`, their fixes in `problems.fixes.list`; the settings are `go.inspections.vet`, `go.inspections.staticcheck` and `go.inspections.onSave` |
| `go.staticcheck.install` | `{}` | Installs staticcheck with `go install` in the background, only when called; `go.inspections.status` reports it as `install` |
| `go.env.get` | `{ path? }` | The project's Go environment (ADR-0298): `shared` (`.zen/go.json`, applied only when `sharedApplies`, a trusted project), `personal` (only on this computer, it wins) and `effective`, each `{ name, value, source }`, what every go command and gopls get before a run configuration's own environment |
| `go.env.set` | `{ scope, environment, path? }` | Replaces one layer (`project` or `personal`) from `NAME=value` pairs separated by `;`; empty clears it. Only `GO*` and `CGO_*`; `GOROOT` and `PATH` refused, `GOENV` and `GOTOOLCHAIN` only `personal`, no URL with a user or password in `project`. gopls restarts. People only (`agentNotAllowed`) |
| `go.env.settings` | `{}` | Settings → Languages & Frameworks → Go → Go Modules as a form (`zen_modules::form`) |
| `toolchain.go.settings` | `{}` | Settings → Languages & Frameworks → Go as a form: `{ items, busy }`, each item a `group`, `text`, `link`, `row`, `choice`, `menu`, `button`, `path`, `check`, `field` or `setting`, with the `action` (`command`, `params`, `value`: the parameter that gets the value chosen) that changes it (`zen_modules::form`) |
| `go.module.versions` | `{ module }` | `{ module, versions, known }`: the versions of a Go module the go.mod checks know, newest first, without retracted ones. It contacts nothing; `known` is false until a `go.mod` that requires the module has been checked. Update a requirement with `problems.fix.apply`, complete one with `editor.completion.start`, open a module's `go.mod` with `navigate.declaration.goTo` |
| `toolchain.rust.settings` | `{}` | Settings → Languages & Frameworks → Rust as the same kind of form: the toolchain, the standard library's sources, rust-analyzer, its Performance settings and On Save; `toolchain.rust.info` gives the toolchain as data, `toolchain.rust.choose { bin }`, `toolchain.rustAnalyzer.choose { path? }`, `toolchain.rustAnalyzer.install`, `toolchain.rustSrc.install` and `toolchain.rustAnalyzer.configure` change it |
| `maven.settings.get`, `maven.settings.set` | `{ project? }`; `{ key?, value?, values?, project? }` | The project's Maven settings (Build, Execution, Deployment → Build Tools → Maven with Importing, Runner and Running Tests), kept per project in `build-tools.json` of the IDE's data folder and used by every Maven run; `set` changes one by dotted `key` (`runner.vmOptions`; `value` null puts the default back) or several with `values`, and answers them all. The pages are the forms `buildTools.settings.form`, `maven.settings.form`, `maven.importing.form`, `maven.runner.form` and `maven.runningTests.form` |
| `gradle.settings.get`, `gradle.settings.set` | `{ project? }`; `{ key?, value?, values?, project? }` | The project's Gradle settings (Build, Execution, Deployment → Build Tools → Gradle): `userHome`, `offline`, `buildAndRun` and `runTests` (`gradle` or `ide`), `distribution` (`wrapper`, `installed`, `custom` with `homePath`), `jvm`, `vmOptions`, `downloadSources`; kept per project in `build-tools.json` of the IDE's data folder and used by every Gradle run. The page is the form `gradle.settings.form` |
| `annotationProcessing.settings.get`, `annotationProcessing.settings.set` | `{ project? }`; `{ key?, value?, values?, project? }` | The project's annotation processing settings (Build, Execution, Deployment → Compiler → Annotation Processors): `enabled`, `processorPath` (`build` or `custom` with `customPath`), `generatedSources`, `options`; kept per project in `build-tools.json` of the IDE's data folder, with `fromBuild`: the processors, options, generated sources folder and Lombok the build files give. The pages are the forms `compiler.settings.form` and `annotationProcessing.settings.form` |
| `gradle.distribution.download` | `{ version? }` | Downloads Gradle (the current release `services.gradle.org/versions/current` names, or `version`) from services.gradle.org, checks its published SHA-256 and unpacks it into the IDE's data folder in a background task: `{ version?, folder?, task? }` |
| `toolchain.rustAnalyzer.settings` | `{ project? }` | rust-analyzer's settings, as its manual names them without the `rust-analyzer.` prefix, nested as objects: `defaults` (the Rust plug-in's, tuned for large workspaces), `ide` (the user's), `project` and `projectFile` (the project's `.zen/rust-analyzer.json`, with `project` its absolute root), `effective` (what rust-analyzer starts with there: the project's over the user's over the defaults) and `problem` when the project's file cannot be read |
| `toolchain.rustAnalyzer.configure` | `{ key, value, project? }` | Sets one setting (`key` dotted, e.g. `check.workspace`; `value` null removes it; text that reads as true, false or a number is taken as that) for the user, or with `project` in that project's `.zen/rust-analyzer.json`; rust-analyzer restarts in every window; answers as `toolchain.rustAnalyzer.settings` |

A product's bundle may mark some of its plug-ins, built-in modules or bundled WebAssembly plug-ins,
as **areas**: shipped but off until chosen, on the product's first start (a dialog with Continue and
Skip) or later in Settings → Plugins → Built-in ([ADR-0073](adr/0073-optional-bundle-plugins-areas.md)).
A product without areas (GoGarden, RustRonin, WebWave, JvmJizo) answers with an empty list.

| Command | Parameters | Result |
|---|---|---|
| `plugins.areas.list` | `{}` | `{ product, offered, dialogOpen, areas, restartRequired }`: each area with `id`, `kind` (`module` or `plugin`), `name`, `description`, `enabled` and `restartRequired`; `offered` says whether the first-start choice was made or skipped |
| `plugins.areas.choose` | `{ areas: [id] }` | The same; switches on exactly these areas and off the others, and records the choice. A bundled plug-in changes at once, a module after `view.ide.restart` |
| `plugins.areas.skip` | `{}` | The same; changes nothing and records that the choice was offered, as **Skip** does |
| `plugins.areas.show` | `{}` | The same; shows the choice dialog over the main window (fails for a product without areas). A choice made by a command closes an open dialog |

| Command | Parameters | What it does |
|---|---|---|
| `diagnostics.log.show` | `{ dryRun? }` | Shows the current log file selected in the file manager; `dryRun` only answers the command line |
| `diagnostics.logs.collect` | `{ path?, noReveal?, wait? }` | Writes a ZIP of the logs, server logs, run output, versions and settings with secrets hidden, in the background unless `wait`; nothing is uploaded. An agent's `path` must be a new file inside the project, not hidden from agents (else `agentNotAllowed` or `redacted`). Answers `{ path, entries, bytes }` (`bytes` with `wait`) |
| `diagnostics.logCategories.get` / `.set` | `{}` / `{ categories }` | The categories of Debug Log Settings, `:trace` after a category writes everything; kept per IDE, applied at once |
| `diagnostics.folders.list` | `{}` | `[{ name, path }]`: settings, data, plug-ins, caches, logs |

`view.collectLogs.open` opens the question of **Help → Collect Logs and Diagnostic Data…** (`collectLogs` in `popups`).
`view.debugLogSettings.open` and `view.specialFiles.open` open the two dialogs of **Help → Diagnostic
Tools** (`debugLogSettings`, `specialFiles` in `popups`).
`view.openSourceLicences.open` opens **Open-source Licences** (`openSourceLicences` in `popups`) and
answers `{ folder, files }`: the bundle's `Licenses` folder and its files, the third-party notices
first; a development build has none (`folder` is null, docs/licences.md).

`ide.errors.list {}` answers the IDE's internal errors since it started (`id`, `message`, `stack`,
`description`), `ide.errors.describe { id, description }` sets what the person was doing,
`ide.errors.report { id }` answers `{ text, url }` (the IDE sends nothing itself) and
`ide.errors.clear {}` forgets them. `get_window_state` answers their number as `internalErrors`, and
the window action `view.ideErrors.open` opens **IDE Internal Errors** (popup `internalErrors`).

The window action `view.ide.restart` (**Restart IDE…**) restarts the IDE: it saves the documents,
keeps the open projects and starts again with the same environment, so an agent connected to
`ZEN_AUTOMATION`'s port reconnects to the new process. An agent's call never restarts by itself:
it always asks the person (`confirmRestart` in `get_window_state`'s `popups`), whatever `confirmed` and
the **Confirm before exiting the IDE** setting say. `press_keys` is refused while that dialog or
**Invalidate Caches…** is open: only the person answers them. The same goes for
`view.ideSettings.restoreDefaults`, whose agent's call opens **Restore Default Settings…** whatever
`confirmed` says, and for the dialogs of `view.ideSettings.import`. A headless IDE refuses both
actions as window-only. The commands `settings.ide.import` and `settings.ide.restoreDefaults` answer
an agent `agentNotAllowed`: both replace settings wholesale, past what `set_settings` refuses, such as
`mcpRedactFiles`. Nor does any command that writes files write into the IDE's data folder for an agent
(`agentNotAllowed`), scratch files aside, and `toolchain.jdk.add`, `remove` and `choose` are a person's:
they choose the program runs start. `toolchain.jdk.download` stays open. The same goes for
`toolchain.go.add`, `choose` and `remove`, `toolchain.gopls.choose`, `toolchain.dlv.choose`,
`toolchain.rust.choose`, `toolchain.rustAnalyzer.choose` and a `toolchain.rustAnalyzer.configure` that
names a program: the refusal comes before the folder's program is started to read its version. The
`toolchain.*.install` commands and the listings stay open. `go.env.set` is a person's too: a Go
environment can run code (`GOFLAGS=-toolexec`); `go.env.get` stays open and shows a user name or
password in a URL as `***` (ADR-0298).

| Command | Parameters | What it does |
|---|---|---|
| `ide.caches.invalidate` | `{ clearLocalHistory?, clearVcsLog? }` | Marks the IDE's derived data for removal at the next start and answers `{ folders, restartRequired }`: the language servers' caches and libraries, with `clearVcsLog` (on by default) the Git log's caches, with `clearLocalHistory` (off by default; refused for agents, `agentNotAllowed`) Local History. Nothing goes until the IDE restarts (`view.ide.restart`) |
| `ide.repair.run` | `{ step? }` | Runs a step of Repair IDE, the first when absent: `refreshFileSystem`, `rescanIndexes`, `resync`, `dropIndexes`. Answers `{ step, did, next }` and replaces the notification that offers the next step |
| `ide.repair.finish` | `{}` | Ends Repair IDE and closes its notification |
| `ide.repair.file` | `{ path }` | The first two steps for one file; answers `{ path, did }` |

`view.invalidateCaches.open` opens **File → Invalidate Caches…** (`invalidateCaches` in `popups`);
`view.repairIde.start` is **File → Repair IDE**: the first step, and the Project tree read again. `view.settings.open { page: "plugins", tab:
"builtIn" }` shows the tab (`marketplace` and `installed` the others).
`view.settings.open { page: "projectStructure", section }` opens Project Structure on a section
(`project`, `modules`, `libraries`, `artifacts`, `sdks` or `problems`). `view.artifacts.build` opens
**Build → Build Artifacts…**, a popup (`buildArtifacts` in `get_window_state`'s `popups`) that lists
the artifacts of `project.structure.artifacts` with **Build**, **Rebuild**, **Clean** and **Edit…**.

`view.settings.open { query }` types `query` into the Settings search, as a person does, in the
interface's language whatever `mcpLanguage` is, and answers `{ search: { query, pages, page,
keymapSearch, plugins: { tab, search } } }`: the pages found in tree order (by their texts, Keymap by
an action's name as shown or in English that contains the query or has a word beginning with one of
its words, Plugins by an installed plug-in), the page shown, and the
Keymap and Plugins pages' own searches, which get the query when those pages are found.

## Find in Files

`search.text.find { query, mask?, directory?, limit? }` searches the project at once and returns
`{ matches, done, error?, reason? }` (`directory` relative to the project or absolute; `reason` with no matches, see [Working cheaply](#working-cheaply-edits-reads-and-the-catalogue)).
Each match is `{ path, line, column, length, text }`: `path` relative to the project (absolute for
a file outside it), `line` and `column` one-based, `column` and `length` in characters of `text`,
the line's text. `search.text.start` with the same parameters searches in the
background, `search.text.results` reads its matches so far and `search.text.cancel` stops it;
`search.text.replaceAll` replaces. `search.replace.start` with the same parameters searches in the
background, and `search.replace.finish` applies it once the search is done (null until then; it
searches again when an open file changed meanwhile). Open files are searched in their editor text, with unsaved
changes. They read only the files the project's trigram index names as candidates, plus open files
and files not indexed yet, with the same results as reading every file
([ADR-0082](adr/0082-trigram-index-for-find-in-files.md)). `search.index.status {}` returns
`{ project, namesReady, ready, indexed, total, files, skipped, trigrams }`: whether the file names
are listed and the text index is built, how many files its first build has read of how many, how
many it holds, how many it left out (binary, not UTF-8, over 2.5 MB) and its distinct trigrams.
`paused` says whether that build is paused. While it builds, the background task `Indexing…` (source
`index`) is in `tasks.background.list`, with its progress; it can be paused, not cancelled.

`search.text.preview { path, line, lines? }` returns `{ first, lines, fromEditor }`: the lines
around a match (9 when `lines` is absent) from the same text, so an open, modified file shows its
editor text and a match's `column - 1` and `length` point into `lines[line - first]` (`path` as a
match gives it, `line` and `first` one-based). The Find in Files
window (⇧⌘F, `view.findInFiles.open`) draws its preview with it.

Matches also carry `offset`, the match's first character as a character offset in the file.
`search.text.edit { path, line, column, endLine, endColumn, text }` replaces the text between two
places (lines and columns from 1, in characters, as the matches give them) of a project file, as typing in the Find in Files
preview does: an open file changes in its editor as one undo step (`fromEditor: true`), any other is
written on disk with a Local History version from before ("Before Edit in Find in Files Preview"),
keeping its CRLF line breaks. With `expected` (the lines the preview showed) and `expectedFrom`
(the line of the first of them) it refuses an edit when the file no longer has those lines there,
so that a change made in the editor or on disk meanwhile is not overwritten; the window then reads
the preview again. A read-only file, and a file the project's settings give an encoding other than
UTF-8, are refused as well.

`search.text.occurrences` takes the parameters of `search.text.start`, with `replacement` (Replace
in Files), `newTab` and `waitMs`, and opens a tab "Occurrences of '…'" in the Find tool window, the
way **Open in Find Window** (⌘↵) does ([ADR-0103](adr/0103-find-window-occurrences.md)). The search
runs in the background as the task "Searching for '…'" (source `search`, `cancellable`, its
`detail` "in <file>…"); `tasks.background.cancel { id? }` stops it as the × in the status bar does.
It answers at once (or after `waitMs`) `{ tab, search, title, query, replacement?, total, files, elsewhere?, done,
more, searching?, pinned, error?, request }`: `files` counts the files with usages, `elsewhere` the
usages in no file (a database's own objects, for db.usages.find), and a title another tab already
has gets a number, "Usages of orders (2)". Past `limit` (the setting `findInFilesTooMany`, 1000;
0 never asks) the search pauses with `more: true` and the window asks **Too Many Results**
(`tooManyResults` in `get_window_state`); `search.results.tooMany { tab?, continue }` goes on or
aborts. New results take the place of the last tab that is not pinned unless `newTab`.

Until 0.38.31 (GoGarden) / 0.2.6 (RustRonin) the command searched synchronously and answered the
final `total` and `more`. An agent that needs the final count now passes `waitMs` (at most 10 000):
the answer comes when the search is over (`done: true`) or paused at Too Many Results
(`more: true`); a search still running after `waitMs` answers `done: false`, and
`search.results.tree { waitMs }` (or `search.results.list`) is read again until `done`. Replace
Selected and Do Replace All refuse while the search runs.

The tabs are `search.results.*`; `tab` is optional everywhere (the tab shown, else the latest):

| Command | Parameters | Does |
|---|---|---|
| `search.results.list` | `{}` | The tabs (as above, with `kind`: `text`, `usages` of Find Usages or `file` of the search bar, and for usages `usages` and `accessKnown`), the tab shown and the Group By `options` |
| `search.results.tree` | `{ tab?, depth?, waitMs? }` | The tree: `nodes` (Targets, then Found occurrences in <scope>), each `{ id, kind, label, detail?, count, excluded?, collapsed?, test?, path?, line?, offset?, end?, highlights?, children? }`, with `selected` and `options` |
| `search.results.setOption` | `{ option, on? }` | Group By and view options: `directoryStructure` ⌃D, `fileStructure` ⌃S, `module` ⌃M, `flattenModules` ⌃O, `directory` ⌃P, `productionTest`, `usageType` ⌃T, `mergeSameLine` ⌃L, `showInComments`, `previewSource`, `showRead` and `showWrite` (Show Read / Write Access, on tabs of usages where the server told the access); kept in the setting `search.results.options` |
| `search.results.exclude` / `include` | `{ tab?, nodes? }` | Exclude (⌫) or Include nodes and every usage under them; the selected node when `nodes` is empty |
| `search.results.replace` | `{ tab?, nodes? }` | Replace Selected (the nodes, or the selected node) or Do Replace All (`nodes: ["found"]`) of a Replace in Files tab, skipping excluded usages; answers `{ files, replacements }` |
| `search.results.select` | `{ tab?, node? }` | Shows a tab and selects a node |
| `search.results.next` / `previous` | `{ tab? }` | Next / Previous Occurrence (⌥⌘↓ / ⌥⌘↑): selects the usage and goes to it; answers `{ node, path, offset, line }` |
| `search.results.expand` | `{ tab?, nodes?, expanded }` | Expands or collapses nodes; every node without `nodes` (Expand All, Collapse All) |
| `search.results.rerun` | `{ tab?, waitMs? }` | Rerun (⌘R) in the same tab |
| `search.results.stop` | `{ tab? }` | Stops the tab's search |
| `search.results.pin` | `{ tab?, on? }` | Pin Tab; answers whether it is pinned |
| `search.results.close` | `{ tab? }` | Closes a tab, stopping its search |

Replace Selected, Do Replace All and `search.text.replaceAll` share one engine: all or nothing, a
read-only file, a file not in UTF-8 (by the project's charset or its bytes), a file outside the
project or one whose matched line changed since the search refuses the whole replacement; a file
that cannot be written midway puts back the files written before it, and open files change only
after every other file is written; an open file changes in its editor as one undo step, any other
is written with a Local History version from before, keeping CRLF. A tab's remaining usages follow
the text a replacement changed, so its usages can be replaced one by one. The window's actions `view.findResults.settings
{ tab? }` (Settings ⇧⌘F: Find in Files opens with the tab's query, options, mask and scope) and
`view.findResults.newTab { on? }` (Open results in new tab) go through `run_action`.

The Find in Files window itself (⇧⌘F) has these actions of the window (`run_action`):

| Action | Parameters | Does |
|---|---|---|
| `view.findInFiles.pin` | `{ on? }` | Pin Window: a pinned window stays open when the editor is clicked and leaves it the keyboard; without `on` it turns over. Answers `{ pinned }` |
| `view.findInFiles.openInFindWindow` | `{ newTab?, all?, waitMs? }` | ⌘↵: runs `search.text.occurrences` with the window's query, options, mask, scope and, in Replace in Files, replacement, and answers its tab at once (after `waitMs` when given); `newTab` overrides **Open results in new tab**, `all` never asks Too Many Results. Past the limit the dialog `tooManyResults` (`get_window_state`) asks: ⏎ Continue lists every match, ⎋ Abort keeps those listed. The window closes unless it is pinned |

The window searches a moment (150 ms) after the query and options stop changing, with `limit` one
over the setting `findInFilesLimit` (100), so that it can say "100+ matches in 29+ files"; it lists
the matches of a line as one row. Its query, replacement and file mask go into the find history
(`search.findHistory.add`) when it opens a match, replaces, opens its results in the Find window or
closes. `search.findHistory.list` returns `{ queries, replacements, masks }`, newest first, at most
50 each, kept in `find-history.json` in the data folder and shared with the search bar;
`search.findHistory.clear` forgets it. The settings `findInFilesLimit` and `findInFilesTooMany`
(`get_settings` / `set_settings`, Settings → Advanced Settings → Find in Files) are the session
settings `search.findInFiles.popupLimit` and `search.findInFiles.tooManyResults`.

## Matching names

Every search by name (Go to File, Go to Class, Go to Symbol, Find Action, Recent Files) uses one
matcher, `zen_index::Pattern`, and one order, `zen_index::Rank`:

- Case is ignored, except that a capital letter starts a new camel hump: `NPE`, `nPE` and
  `nullpoex` find `NullPointerException`, `mgo` finds `main.go`, `IS` does not find `Items`. Small
  letters go on in the same word or jump to the start of a later one (`_`, `.`, `-`, a capital, a
  digit start words).
- `*` is any characters (`i*re` finds `ItemStore`); a space is a word boundary (`dup lin`); a space
  at the end means the name ends there (`item ` finds `Item`, not `ItemStore`; a file's extension
  may be left out: `main ` finds `main.go`).
- `/` puts folders before a file name (`internal/ca`); for symbols `.`, `::` and `/` put what a
  symbol belongs to before its name (`model.Store.get`).
- A `:line` or `:line:column` suffix (`main.go:42:7`) is cut off and returned apart.
- Order: how well the name matches first: the exact name (or a file name without its extension),
  then a prefix, then camel humps, then a match inside a word; within each kind the project's before
  the SDK's and the libraries' (`Rank::in_project`), then how well within the kind, then the more
  recent, then the shorter name. `Cart` puts the project's `CartStore` before the SDK's
  `CartPool`, but an SDK `Cart` (exact) before the project's `CartStore` (prefix).

Results carry `ranges`, the matched characters as `[start, end)` character indexes, which the
popups mark.

| Command | Parameters | Result |
|---|---|---|
| `navigate.names.match` | `{ query, names, mode?, libraries? }` | `{ matches, line?, column? }`: the names that match, best first, each `{ name, kind, degree, ranges }` (`kind` `exact`, `prefix`, `humps` or `middle`; `degree` how well within it). `mode` is `names` (one part, the default), `files` (paths relative to a project) or `symbols` (qualified names such as `model.Store.Get`); `libraries` lists the names that are not in the project, which rank after the project's among matches of the same kind |
| `navigate.file.goTo` | `{ query, limit? }` | `{ files, indexing, line?, column? }`: each file `{ path, relative, kind, ranges }`, `ranges` in `relative`; `line` and `column` from a `:line[:column]` suffix |
| `navigate.recent.files` | `{ query? }` | the files shown last, most recent first, `{ path, relative, open, ranges? }`; with `query` only those it matches, in the same order |

`find_action` answers each action with `ranges` in its `name` too (empty when only its English name
matched).

## Search scopes

Find in Files and the Classes, Files and Symbols tabs of Search Everywhere search in a scope
([ADR-0083](adr/0083-search-scopes.md)).

| Command | Parameters | What it does |
|---|---|---|
| `search.scope.list` | `{ everywhere? }` | The scopes in the order the lists show them (`id`, `name`, `group`: predefined, module or named, a named scope's `pattern`, `error` and `own`), the `modules`, and the `selection` remembered; `everywhere` leaves out Current File, as Search Everywhere does |
| `search.scope.select` | `{ area, module?, directory, recursive, scope, everywhere }` | Remembers Find in Files' scope row (`project`, `module`, `directory` or `scope`) and Search Everywhere's scope; saved with the project |
| `search.scope.files` | `{ scope?, pattern?, limit? }` | The files a scope holds, or those a pattern in the scope language matches: `{ files, total }`; an unusable pattern is an error saying why |
| `search.scope.filter` | `{ scope?, paths }` | Which of the absolute `paths` the scope holds |
| `search.scope.save` | `{ name, pattern, previous? }` | Adds or changes one of the project's own named scopes (`.zen/scopes.json`); `previous` renames |
| `search.scope.delete` | `{ name }` | Removes one of the project's own named scopes; those of `.idea/scopes` are read-only |

`search.text.find` right after a project opens waits up to 10 s for the project's files to be
listed; if they still are not, it answers `done: false` with `reason: searching`, never `noMatch`.

`search.text.start`, `search.text.find` and `search.text.replaceAll` take `scope` (an id from the
list, Project Files when absent), `directory` (relative) and `recursive` (true when absent); Replace
in Files refuses files outside the project. The pattern language: `file:` patterns with `*`, `?`
and `//` (any folders below; `file:*.go` matches names, `file:internal//*` paths), `file[module]:`,
`&&`, `||`, `!`, parentheses and `$Name`.

**Grouped matches.** `search.text.find { group: true }` answers `files` instead of `matches`: each
file's `path` once, with its matches (`line`, `column`, `length`, `offset`, `text`) under it, the
cheaper form when a file has several matches. With `compact: true` it answers `rows`: a `path` row,
then a `line:column text` row for each of the file's matches; `compact` alone gives
`path:line:column text` rows. Without either, the answer is as before.

`search.everywhere.query` takes `scope` as well (an id of `search.scope.list { everywhere: true }`):
the files and declarations of its answer are kept to it, as the popup does on its tabs other than
All with the scope chosen last (`selection.everywhere`). Any scope but Project Files also searches
the SDK and the libraries; Include non-project items on All is All Places, and switching it
(or ⇧⇧ in the open popup) chooses All Places or Project Files for the other tabs too.

## Go to Class

`navigate.class.goTo { query, limit?, waitMs? }` finds the project's types by name with the
language servers' `workspace/symbol`, kept to the type kinds (class, interface, struct, enum):
`{ classes, pending }`, each type with `name`, `kind`, `container` (for Go, the package path),
`path`, `offset` (a character offset of its name, for `navigate.place.goTo`), one-based `line` and
`column`, `inProject` and `ranges` (in `name`), ranked as in [Matching names](#matching-names). It starts the project's servers even when no file of theirs is open; `pending` is
true while one starts or searches, and `waitMs` (at most 10 000, or 120 000 while a server loads the project) waits for them. The Classes tab of
Search Everywhere (⌘O, `view.gotoClass.open`) calls it every frame.

## Go to Symbol

`navigate.symbol.goTo { query, limit?, waitMs?, types? }` finds every declaration by name
(functions, methods, fields, constants, variables and, unless `types` is false, types) with the
same search as Go to Class: `{ symbols, pending, reason?, progress? }` (`reason` says why nothing came or more may come), each with `name`, `qualifiedName` (with the type
it belongs to, as `Store.Get`, in every language: a Rust method of `IdeApp` is `IdeApp.route_call`), `kind`, `container`, `path`, `offset`, `line`, `column` and
`inProject` and `ranges` (in `qualifiedName`). `limit` is 50 by default and at most 500.

Both commands share one search: one `workspace/symbol` request per query and server (two for a
dotted query, see below), whichever asks.
A new query cancels the requests of the last one; without `waitMs` it is sent only once it has
stayed the same for 120 ms, so typing in the popup does not flood the server; until its answers
come, the last query's symbols are matched against it, so the list does not blink. The answers are
matched again here by the shared matcher ([Matching names](#matching-names)), so every server ranks
alike. The servers get only what they understand: the query's letters and dots, without `*`,
spaces and a `:line` suffix (`i*re` asks for `ire`, `item store` for `itemstore`), since gopls
takes `*` as a letter and matches spaces its own way; the matcher then keeps and ranks the right
names and marks their letters. Queries that differ only there (`i*re`, `ire`) share one search. A symbol two answers both
hold is listed once. The ranking is made once per query and answer, not on every call. Names come
without their package and receiver syntax: gopls's `model.Store.Get`, `(*Store).Get` and
`Store[T].Get` are all `Store.Get`. A dotted query (`Store.get`, `model.Store.get`, `::` works too) matches its last part against the
name and the parts before, in order, against what the symbol belongs to: its type, then the
segments of its package path. gopls (known by its `serverInfo`) gets the dotted query as typed,
since it understands that form, and its last part (`get`) only when it finds nothing; any other
server gets both at once, as many do not match dotted names. The answers are then matched against
the whole query. Symbols of files only a plug-in outlines are not searched. The Symbols tab of Search
Everywhere (⌥⌘O, `view.gotoSymbol.open`) calls it every frame; the All tab lists the first few
symbols that are not types (types are its Classes).

## Search Everywhere

The popup is built from contributors ([ADR-0081](adr/0081-search-everywhere-contributors.md),
[ADR-0095](adr/0095-search-everywhere-toolbar.md), [ADR-0102](adr/0102-search-everywhere-commands-calculator-shortcuts.md)):
Commands (`commands`, the `/` commands), Calculator (`calculator`), Recent Files (`recentFiles`, on
All with nothing typed), Classes (`classes`, titled Types where the language says so), Files (with
folders, target `directory`), Symbols, Actions and Text (`text`, lines of the project's files in the
tab's scope), and those of built-in modules (`ModuleHost::search_contributor`). With nothing typed
only Recent Files lists anything. Text is a fallback ([ADR-0105](adr/0105-search-everywhere-text-tab.md)):
on All it is searched only once every other contributor is done and they found fewer than 6 items
in all, and its group comes last; `searchTextOnAll` (Advanced Settings, on by default) turns that
off. On All a query that
starts with `/` is a command, which only Commands answers (50 items, not 6): `/` and the start of a
command (`/ap`) list the groups of Settings (`/appearance`, `/keymap`, `/editor`, `/plugins`, `/vcs`,
`/build`, `/languages`, `/tools`, `/advanced`), each with target `{"kind": "settings", "page"}`
(`run_action` `view.settings.open { page }` opens it); `/plugins` lists after its group the built-in
plug-ins and the installed ones, the words after it (`/plugins dock`) filtering them, each with
target `{"kind": "switch", "on", "command", "params"}`: `dispatch_command` with that `command`
(`plugins.module.enable`, `plugins.plugin.disable`...) and `params` switches it to the other state,
as choosing the row does (the popup stays open). An arithmetic expression on All (`2 + 3 * 4`,
`(1.5 + 2) ^ 2`, `17 % 5`) gets a first group, Calculator, with one item `2 + 3 * 4 = 14` and
target `{"kind": "value", "value": "14"}`; choosing it copies the value (to the clipboard history
too) and closes the popup. A comma is a decimal separator only in a language that writes one: in
the popup the interface's, over MCP the agents' (`mcpLanguage`), so `1,5 * 3` has no value for an
agent that gets English.

- `search.everywhere.query { tab?, query, includeNonProject?, limit?, more?, scope?, matchCase?,
  wholeWords?, regex?, mask?, typed?, waitMs? }` searches as the popup does (`typed: true` reads the
  query in the other keyboard layout too, as the popup does for a query typed rather than pasted:
  `ьфшт` finds `main` after what `ьфшт` itself finds) and answers what it lists: `{
  tab, groups, pending, held, waiting }`. `tab` is `all` (by default) or a contributor's id; each
  group has its `contributor`, the Fluent id of its `title`, `more` (true when the contributor
  found more than it lists: the popup ends the group with more…) and its `items`; `more` of the
  request (`{"files": 1}`) is how many times more… is chosen in each group, each time listing 50
  more of its items; when only `more` grows, only the groups it grew in are searched again and the
  rest stay as they are. Each item has `name`, `detail` (an action's group, or its English name
  when only that matched), `hint`, `ranges` and `detailRanges` (the characters of `name` and of
  `detail` the query matched, `[start, end)` pairs as in [Matching names](#matching-names)),
  `inProject`, `available` and `target`: `{"kind": "file", "path", "line"?, "column"?}` (`line`
  and `column` from a `:line[:column]` suffix of the query), `{"kind": "place", "path", "offset",
  "line", "symbol"}` for `navigate.place.goTo`, `{"kind": "directory", "path"}` for
  `view.project.select`, or `{"kind": "action", "action", "command", "opens", "params"}` for
  `run_action`. `includeNonProject` (false by default, as the popup's checkbox) adds the SDK's and
  the libraries' types and symbols. `pending` is true while a contributor searches; `held` while
  the new results are held back and `groups` are still the last query's (until the primary
  contributors, Classes, Files and Actions, are done or have 8 items, or 2000 ms; with
  `searchWaitForAll` until every one is done); the command waits that hold out, so its answer is
  the list the popup settles on. `waitMs` (at most 10 000) also waits until none is pending. Calls
  with the same parameters continue one search, as the popup's frames do. On `text` (and Text on
  All) each item is a line of a file, `hint` its `file:line`, `ranges` the matches in it and
  `target` `{"kind": "file", "path", "line", "column"}` at the first match; Find in Files' engine
  searches the tab's `scope`, stopping at four matches per item asked, and a new query cancels the
  last one's search. `matchCase`, `wholeWords`, `regex` and `mask` (`*.go, !*_test.go`) say how
  text matches; each one left out is the popup's choice (`search.everywhere.options`).
- `search.everywhere.contributors` lists them, lighter first: `id`, `title`, `hasTab`, `weight`,
  `primary` and `fallback` (true for Text).
- `search.everywhere.options` answers the popup's options and `search.everywhere.setOptions`
  changes those given: `preview`, `previewHeight`, `excludedContributors` (the Filter of All),
  `excludedFileTypes` (Files), `excludedLanguages` (Types and Symbols),
  `includeDisabledActions`, and the Text tab's `textMatchCase` (Cc), `textWholeWords` (W),
  `textRegex` (.*) and `textMask` (the file mask, empty for every file). They are kept in `search-everywhere.json` and applied to the search, so
  `search.everywhere.query` lists what the popup lists.
- `search.everywhere.filters { tab }` lists the Filter of `all`, `files`, `classes` or `symbols`:
  `{ id, name, on }`, the contributors, then `directory` and the file types, or the languages.
- `search.everywhere.openInFind { tab?, query, includeNonProject?, scope?, waitMs? }` searches
  (waiting 5000 ms by default) and puts the results on a new tab of the Find tool window, which
  shows; it answers `{ search, tab, title, query, groups }` and is refused on `actions`.
- The window state's popup is `searchEverywhere:<tab>`; while it is open, `press_keys` moves in its
  list as people do: `↓`, `↑`, `⌘↓` and `⌘↑` (the end and the start), `⏎` (on more…, the rest of
  the group). `⌥⏎` on an action opens the Keyboard Shortcut dialog (popup `keyboardShortcut`), which
  takes every key until ⎋ or OK; agents give the shortcut with `ui.keymap.setShortcut` or
  `set_keymap` instead.
- `ui.keymap.setShortcut { action, keystrokes? }` sets an action's keystrokes in the keymap in use
  (an empty list removes them, none brings back the defaults), also for a registered command that
  has no shortcut of its own: the keymap then holds a global binding of that name, which runs what
  choosing the command in Find Action runs. `ui.keymap.abbreviations` lists the abbreviations of
  the actions (Settings → Keymap → Add Abbreviation, kept in `abbreviations.json`, belonging to no
  keymap), and `ui.keymap.addAbbreviation` / `ui.keymap.removeAbbreviation { action, abbreviation }`
  change them. Find Action lists an action first when the query is one of its abbreviations or the
  start of one, and finds an action by its synonyms (`synonyms.ftl`, in the interface's language
  and in English; `get_keymap` lists them), showing the synonym matched in `detail`.
- `search.history.list` answers `{ queries, actions }`, the queries a result was chosen with (50)
  and the actions run from the popup (20), newest first, kept in `search-everywhere.json` in the data
  folder; `search.history.clear` forgets them. Recent actions rank first in Find Action.
- `run_action` `view.gotoText.open` (⌥⇧⌘E, Navigate → Text…) opens the popup on Text, or closes it
  when it is open there; the window state's popup is then `searchEverywhere:text`.
- `search.findHistory.list` answers `{ queries, replacements, masks }`, newest first (50 each),
  kept in `find-history.json` in the data folder: the file masks are the ones Find in Files and the
  Text tab used (a mask joins when a line found with it is opened), and the Text tab's mask combo
  lists them. `search.findHistory.add { query?, replacement?, mask? }` puts them first and
  `search.findHistory.clear` forgets them.

## Lookups at a place

The language commands that take `{ path?, offset? }` (`navigate.usages.find`, `navigate.usages.show`,
`navigate.declaration.goTo`, `navigate.implementation.goTo`, `navigate.typeDeclaration.goTo`,
`navigate.superMethod.goTo`, `editor.declarationPeek.show`,
`editor.documentation.show`, `editor.parameterInfo.show`, `refactor.rename.prepare`,
`refactor.rename.preview`, `refactor.rename.apply`, `navigate.hierarchy.call`, `navigate.hierarchy.type`)
accept a file that is not open in the editor;
`offset` is a character offset, and `path` without `offset` still needs the file open (its caret).
Such a file is lent to its language server: opened there with its text on disk, without an editor
tab, kept in step with the disk while a lookup waits on it, and closed in the server once no lookup
does. When its server is not running yet the command fails with "The language server is not
running." and starts it; call again once `language.servers.status` says it runs. A path that does
not exist fails with "There is no file …".

## Show Usages

The Show Usages popup (⌥⌘F7, the guide's [Show Usages](guide/06-go.md#show-usages)) is driven by
these commands; the window shows the popup whenever `open` is true. Its state is
`{ search, generation, open, pending, path, offset, symbol, kind, declaration, declarationRelative,
scope, options, settings, total, found, files, more, error }`: `kind` is what the symbol is
(`variable`, `parameter`, `constant`, `function`, `method`, `type`, `struct`, `interface`, `class`,
`field`, `property`, `package`, `module` or `symbol` when nothing says), `scope` one of
`currentFile`, `openFiles`, `projectFiles`, `allPlaces`, `options` the toggles `{ mergeSameLine,
read, write, generated, imports, comments, preview }`, `total` the usages that pass the scope and
the toggles, `found` all the server found, `more` the rows not listed yet. `files` holds the rows
listed, by file: `{ path, module?, rows }`, `path` relative to the project (absolute outside it),
and a row is `{ line, column, start, end, preview, highlights, access, comment?, count? }`,
`access` being `read` or `write`, `highlights` the usages' character ranges in `preview`, `comment`
true for an occurrence in a comment and `count` how many usages a merged row holds (left out when
one). The popup reads the rows from the window's state (`show_usages::view`).

| Command | Parameters | Result |
|---|---|---|
| `navigate.usages.show` | `{ path?, offset?, waitMs?, scope? }` | Show Usages (⌥⌘F7): asks the server for the references at the place (the caret by default) and opens the popup with the first 100 rows; at the same place while it is open, it widens the scope instead (or sets `scope`) |
| `navigate.showUsages.get` | `{ waitMs? }` | the popup's state, the server's answer taken if it has come |
| `navigate.showUsages.more` | `{}` | lists 100 more rows |
| `navigate.showUsages.setOptions` | `{ mergeSameLine?, read?, write?, generated?, imports?, comments?, preview?, scope?, settings? }` | changes the toggles given, the scope and whether the settings panel is open |
| `navigate.usages.settings` | `{ path?, offset?, scope? }` | Find Usages Settings… (⌥⇧⌘F7): opens the Find Usages dialog (`findUsagesSettings` in `get_window_state`) for the symbol at the place, or for the Show Usages popup while it is open (it closes the popup; Find shows it again); answers the dialog's state |
| `navigate.showUsages.openInFindWindow` | `{}` | Open in Find Tool Window (⌥F7 in the popup): closes it and runs `navigate.usages.find` at the same place, in the popup's scope; its result |
| `navigate.showUsages.close` | `{}` | closes the popup, cancelling its search |

### Find Usages in the Find tool window

`navigate.usages.find { path?, offset?, waitMs?, includeDeclaration?, scope?, newTab? }` (⌥F7)
asks the server for the references and, with its answer, opens a tab "Usages of <symbol>" of the
Find tool window ([ADR-0118](adr/0118-find-usages-tab.md)): a tab of the same kind as Find in
Files' (`kind: "usages"` in `search.results.list`), so `search.results.tree`, Group By
(`search.results.setOption`), exclude, select, next and previous, expand, pin, close and rerun
(which asks the server again) work on it as they do on text occurrences. While the server has not
answered, the background task "Finding usages of '…'" runs; the answer's `tab` (in
`navigate.usages.find` and `navigate.usages.list`) is the tab once made. The tree's top nodes are
Targets ("Variable Version") and "Usages in <scope>"; with Usage Type the usages are under "Value
read" and "Value write" where the server told the access (`textDocument/documentHighlight`, asked
once per file, for at most 500 files, in the background with 8 requests at a time, each given up
after 5 s; closing, stopping or rerunning the tab cancels them), "Usage in import", "Usage in
comments" or "Unclassified". The tab is made at once and its usages move from Unclassified to the
reads and writes as the server answers: `accessPending` says it is still asked, and `waitMs` (of
`navigate.usages.find`, `navigate.usages.list` and `search.results.tree`) also waits for that.
`accessKnown` says the server told some access: `showRead` and `showWrite` then filter the reads and
the writes; a usage of unknown access is never filtered. The tab's `usages` is what was looked for
`{ path, offset, symbol, kind, scope }`; Rerun and `navigate.usages.openRecent` find the symbol
again after edits by the text of its line, else nearest to where it was. `navigate.usages.find`'s
`files` and `total` are the server's whole answer; the tab keeps the usages in `scope`.

The Find Usages options are kept for the window in the setting `navigate.usages.options`
`{ scope, newTab, skipSingle }`: the scope (`projectFiles`, `allPlaces`, `openFiles`,
`currentFile`), Open in new tab, and Skip results tab with one usage (a single usage opens in the
editor and no tab is made; the answer then has no `tab`).

| Command | Parameters | Result |
|---|---|---|
| `navigate.usagesSettings.get` | `{}` | The dialog's state `{ open, generation, target?, targetName?, options, showUsages, scopes }`; the window's options while it is closed |
| `navigate.usagesSettings.set` | `{ scope?, newTab?, skipSingle? }` | Changes the dialog's options while it is open (kept only by Find), else the window's options at once |
| `navigate.usagesSettings.find` | `{ waitMs? }` | Find: keeps the dialog's options, closes it and finds (the Find tool window, or the Show Usages popup it was opened for) |
| `navigate.usagesSettings.cancel` | `{}` | Closes the dialog, keeping nothing |
| `navigate.usages.recent` | `{}` | Recent Find Usages: `{ searches }`, newest first, each `{ index, title, relative, scopeName, path, offset, symbol, kind, scope }` |
| `navigate.usages.openRecent` | `{ index?, waitMs? }` | Runs a recent Find Usages again (the symbol looked for on its line, else near its old place, if the file changed); its result |

The action `view.recentFindUsages.open` (Edit → Find Usages → Recent Find Usages) opens the popup
of the recent searches (`recentFindUsages` in `get_window_state`) and answers them.

The server is asked with `includeDeclaration: true`: the declaration is the answer's place that the
language server's semantic tokens mark as a definition or that a symbol of the file's structure
starts at; it is left out of the rows and gives `kind` and `declaration`. Whether a usage writes
comes from the server's `documentHighlight` when the editor has it for that file, else from the
text after the name (`=`, `:=`, `+=`, `++`...). A file is generated when a comment among its first
40 lines holds `Code generated` and `DO NOT EDIT`, or `@generated`; a usage is an import on a line
that starts an `import`, `use` or `from` statement (not `from := …`) or inside an `import ( … )` block; with `comments`, the name's whole-word
occurrences in `//` and `/* */` comments of the files with usages are listed too. The module is the
folder of the nearest `go.mod`, `Cargo.toml`, `package.json`, `pyproject.toml` or `pom.xml`, else
the project's folder.

## Call and type hierarchy

Call Hierarchy (⌃⌥H) and Type Hierarchy (⌃H), the guide's
[Call and type hierarchy](guide/06-go.md#call-and-type-hierarchy), come from the language server:
the client declares `textDocument.callHierarchy` and `textDocument.typeHierarchy`, and a server that
offers neither `callHierarchyProvider` nor `typeHierarchyProvider` (gopls before v0.19 has no type
hierarchy) is reported, not failed: the state says `supported: false` and why in `error`, and
nothing is asked. A `MethodNotFound` answer is reported the same way; another error of the server at
the place (gopls: "identifier not found") is "nothing to show". The hierarchies of a window are the
tabs of the Hierarchy tool window ([ADR-0104](adr/0104-hierarchy-tool-window-tabs.md)): a new one
replaces the selected tab unless it is pinned (then another unpinned tab, else a new tab), cancelling
the requests of the hierarchy it replaces, and is selected; a pinned tab stays and keeps working. The
commands that act on a hierarchy take `tab` (the selected one by default); `expand`, `collapse` and
`cancel` with a node find its tab. A cancelled request's answer that comes later is dropped, and a
request sent to a server that has since restarted is never cancelled in the new one (its node shows
the error and can be expanded again).

Its state is `{ open, tab, pinned, kind, direction, scope, sort, singleClick, symbol, pending,
supported, roots, nodes, hidden, error, tabs, active }`: `tab` is the tab's id and `pinned` whether
it is pinned; `kind` is `call` or `type`; `direction` `callers` or `callees`, `supertypes` or
`subtypes`; `scope` `project` (the project's files that are not tests), `test` (its test files,
`*_test.go` and the other names of Project Test Files) or `all` (libraries and the SDK too, the
default); `sort` whether the children are sorted by name (Sort Alphabetically); `singleClick`
whether a click on a node in the tool window goes to its code (scope, sort and single click are kept
for the next hierarchy and saved with the project's layout); `roots` the ids of what is at the
place; `nodes` the roots and the children in the scope of the nodes asked for and not collapsed, in
the tree's order; `hidden` how many children the scope hides; `tabs` every tab (`{ tab, kind,
direction, symbol, pinned }`) and `active` the selected one. A node is `{ id, name, kind, detail,
path, range, selection, location, parent, calls, children, collapsed, recursive, pending, error }`:
`detail` is what the server tells besides the name (gopls: the package and the file,
`example.com/shop/cart • cart.go`), `range` the declaration's character offsets, `selection` the
name's place (`{ path, start, end, line, column, preview, highlight }`, for `navigate.place.goTo`),
`location` `project`, `test` or `library`, `calls` the call sites between the node and its parent
in the caller's file (call hierarchy only), `children` `null` until the node is expanded,
`collapsed` whether its children are hidden (they are then not in `nodes`), `recursive` whether the
node is one of its own ancestors (a recursive call: never asked about, `children` empty). Node ids
are never reused in a window, whatever the tab: the nodes of a hierarchy that was replaced,
refreshed, closed or turned the other way keep theirs, and expanding such an id is an error.

| Command | Parameters | Result |
|---|---|---|
| `navigate.hierarchy.call` | `{ path?, offset?, waitMs?, direction?, scope? }` | Call Hierarchy: `textDocument/prepareCallHierarchy` at the place (the caret by default), then the children of each root (`callHierarchy/incomingCalls` for `callers`, the default, or `outgoingCalls` for `callees`) |
| `navigate.hierarchy.type` | `{ path?, offset?, waitMs?, direction?, scope? }` | Type Hierarchy: `textDocument/prepareTypeHierarchy`, then `typeHierarchy/subtypes` of an interface or `supertypes` of another type, unless `direction` says |
| `navigate.hierarchy.expand` | `{ node, waitMs? }` | shows the node's children: asks for them once, in the hierarchy's direction, and undoes a collapse; a recursive node is never asked about |
| `navigate.hierarchy.collapse` | `{ node?, tab? }` | hides the node's children, or every node's of the tab (Collapse All), keeping what was answered |
| `navigate.hierarchy.expandAll` | `{ tab?, depth?, waitMs? }` | Expand All: undoes the collapses and asks for the children of every node in the scope down to `depth` levels below the roots (3 by default, at most 10) with at most 256 requests, recursive nodes left out |
| `navigate.hierarchy.get` | `{ tab?, waitMs? }` | the tab's state, the answers that came taken in |
| `navigate.hierarchy.refresh` | `{ tab?, waitMs? }` | Refresh: the same kind, direction and scope asked again after edits, the root found by its name nearest to where it was, and the nodes that were expanded (matched by names and files) expanded again, with new ids |
| `navigate.hierarchy.setOptions` | `{ tab?, direction?, scope?, sort?, singleClick?, waitMs? }` | a new scope or sorting filters and orders the nodes had at once, asking nothing; a new direction keeps the roots and asks for their children again; with no tab open, only the options for the next hierarchy |
| `navigate.hierarchy.cancel` | `{ node?, tab? }` | cancels the requests not answered yet, the node's or all the tab's (`$/cancelRequest`); a cancelled node can be expanded again |
| `navigate.hierarchy.pin` | `{ tab?, pinned? }` | Pin Tab (`pinned` true by default) or unpin: a new hierarchy never replaces a pinned tab |
| `navigate.hierarchy.select` | `{ tab }` | selects a tab, as a click on its title does |
| `navigate.hierarchy.close` | `{ tab? }` | closes the tab, cancelling its requests, and selects the tab before it, else the one after |

The Hierarchy tool window (right, no shortcut: View → Tool Windows → Hierarchy, `view.toolWindow.hierarchy`)
draws the selected tab and runs these commands from its toolbar and tree; Call Hierarchy (⌃⌥H) and
Type Hierarchy (⌃H), from the editor or by an agent, open it. A double click, ⏎ or F4 on a node goes
to its name through `navigate.place.goTo`, which leaves a place for Back.

gopls leaves out calls through an interface or a function value, and lists types by name only
(no aliases).

## Highlighted usages

The usages of the symbol at the caret, as the editor highlights them (the guide's
[The name at the caret](guide/02-editor.md#the-name-at-the-caret)). Ranges are character offsets
`{ start, end, kind }`, `kind` being `read` or `write` (the language server's
`DocumentHighlightKind`) or `text`; an answer is `{ path, source, pending, ranges }`, `source` being
`server` (`textDocument/documentHighlight`), `text` (the name's whole-word occurrences, for a file
whose server does not run or does not highlight, or when its request fails), `selection` (the
selected text's occurrences anywhere) or `none`. While `pending`, call again or pass `waitMs`.

| Command | Parameters | Result |
|---|---|---|
| `editor.highlights.get` | `{ path?, offset?, waitMs? }` | the highlights at `offset` (the caret by default; without `offset`, a non-empty selection gives its text's occurrences); asked once per file version, place and selection, the answer kept |
| `editor.highlightUsages.toggle` | `{ path?, offset?, waitMs? }` | Highlight Usages in File (⇧⌘F7): pins the highlights at the place; at a place inside the pinned ones it unpins them and answers with nothing |
| `navigate.usages.findInFile` | `{ path?, offset?, waitMs? }` | Find Usages in File (⌘F7): pins them, never unpins |
| `editor.highlightUsages.list` | `{ waitMs? }` | the pinned highlights; nothing once cleared or once their file changed |
| `editor.highlightUsages.next`, `editor.highlightUsages.previous` | `{}` | moves the caret to the start of the next or previous pinned usage of the active file (else of those at the caret), round the ends; the range, or null |
| `editor.highlightUsages.clear` | `{}` | unpins them (Escape, `editor.caret.keepPrimary`, does it too); whether there were some |

The setting `highlightUsagesAtCaret` (`set_settings`) stops the editor highlighting the usages at the
caret by itself; the commands still answer.

## Live templates

Live templates are commands of the project window (the welcome window's settings reach the
`editor.templates.*` ones too); the guide's [Live templates](guide/02-editor.md#live-templates)
says what the user sees. Offsets are character offsets.

| Command | Parameters | Result |
|---|---|---|
| `editor.templates.list` | `{ language? }` | each template: `template` (`language`, `abbreviation`, `description` as declared, `text`, `variables` `[{ name, default, expression?, skipIfDefined? }]`, `imports`, `contexts?` of `code`, `comment`, `string`, `enabled`), `descriptionText` (in the interface's language), `builtin`, `modified`, `default?` (the built-in one as declared), `plugin?` (the plug-in that declares it: read-only but for `enabled`); with `language`, those that work in its files (its own and `other`) |
| `editor.templates.expand` | `{ name, path?, offset? }` | inserts the template `name` of the file's language at `offset` (the caret by default), replacing the abbreviation when it is there, and starts filling it in: `{ active, template, variable, index, count, ranges }`, `ranges` being the occurrences of the selected variable as `[start, end]`; `active` is false for a template without variables, the caret then at its end |
| `editor.templates.next`, `editor.templates.previous` | `{}` | selects the next or previous variable of the template being filled in (after the last, `next` ends it at its end); the same answer, `active: false` once it ended |
| `editor.templates.end` | `{}` | ends the template being filled in, the caret at its end |
| `editor.templates.insert` | `{}` | ⌘J: a completion of the active file's templates (`editor.completion.list`, items of kind `template`); `editor.completion.accept` puts one in |
| `editor.templates.set` | `{ template, previous? }` | adds a template or replaces the one with the same `language` and `abbreviation` (a built-in one becomes `modified`; `enabled: false` switches it off); `previous` renames a user's template. Saved in `templates.json` |
| `editor.templates.remove` | `{ language, abbreviation }` | removes a user's template; a built-in one fails |
| `editor.templates.reset` | `{ language?, abbreviation? }` | `{ reset }`: how many changed built-in templates are back as declared |
| `editor.postfix.list` | `{ language? }` | `{ enabled, templates }`: whether postfix completion is on, and each postfix template (`template` with `language`, `key`, `description`, `text` holding `$EXPR$`, `variables`, `imports`; `descriptionText`, `enabled`, `plugin?`) |
| `editor.postfix.set` | `{ language?, key?, enabled }` | switches a postfix template, a language's, or (with neither) postfix completion as a whole; saved in `templates.json` |
| `editor.postfix.expand` | `{ key, path?, offset? }` | replaces `expression.` and what is typed of the key before `offset` with the postfix template and starts filling it in, as `editor.templates.expand` answers |
| `editor.statement.complete` | `{ path? }` | ⇧⌘⏎: finishes the statement at the primary caret by the language's `StatementRules` (closes brackets and a string, adds a block with the caret inside or the terminator, or a new line); the selections |
| `editor.completion.smart` | `{}` | ⌃⇧Space: a completion like `editor.completion.start` that keeps the server's `preselect` items, or, when it marks none, its items without keywords, snippets and templates |

`editor.edit.tab` goes to the next variable while a template is filled in, else tries the file's
plug-in (`editor.emmet.expand`), then a template, then indents; `editor.line.outdent` (⇧Tab) goes
back, `editor.edit.newline` (Enter) goes on like Tab, and `editor.caret.keepPrimary` (Escape) ends
it. Tab expands only at the primary caret when no caret has a selection, and only when the
abbreviation, a whole word, is in a place the template applies in by the file's highlighter
(`contexts`, code when none); then it tries a postfix template whose key ends `expression.key`. A template's
`imports` (e.g. `["fmt"]`) are added through the language module's `AddImport` in the same undo step. Completion
lists the templates of the file's language that apply at the caret as items of kind `template`, and
after `expression.` the postfix templates as items of kind `postfix`; a server's snippet with the
same label is left out. Variables with an `expression` are worked out in Tab order and again when Tab
leaves a variable they read; `skipIfDefined` ones with a value are not visited. Typing the opening of
a language's doc comment above a declaration (`editor.edit.type`, or `editor.edit.newline` for a stub
on Enter) puts its stub as a template, in an undo step of its own. Modules and plug-ins
(`liveTemplates` in `plugin.json`, [plugins.md](plugins.md)) declare all of these as data
([ADR-0218](adr/0218-editable-live-templates-postfix-and-statement-completion.md)).

### Surround With

Surround With (⌥⌘T) and Surround with Live Template (⌥⌘J) are two commands over the primary
selection, or the statement at the primary caret without one (the caret line down to the line that
closes the brackets it opens, or the whole statement when the caret is inside one: in its brackets,
in a literal, in a multi-line string, on its closing `}`, on a continued line; strings and comments
known to the file's highlighter do not count, [ADR-0119](adr/0119-surround-with-as-live-templates.md));
the guide's [Surround With](guide/02-editor.md#surround-with) says what the user sees.

| Command | Parameters | Result |
|---|---|---|
| `editor.surround.list` | `{ path?, templatesOnly?, preview? }` | `{ path, items }`: the language module's surrounders (Go: `if`, `ifElse`, `for`, `func`, and with a selection `parentheses`, `not`), then `editor-fold` and `region` in the file's comment syntax, then the live templates whose text holds `$SELECTION$` (only those with `templatesOnly`). Each item: `index`, `mnemonic` (`1`–`9`, `0`, `A`–`Z`), `name`, `template`, `title`, `description` (a template's), `range` (the characters it wraps, `[start, end]`) and, with `preview`, the changed lines as they would read |
| `editor.surround.apply` | `{ name, template?, path? }` | wraps the code with the surrounder `name`, or the live template `name` with `template: true`, as one undo step; the answer is that of `editor.templates.expand`: the variable the caret is on (an `if`'s `CONDITION`), Tab goes on as in a template |

A statement surrounder (and a template whose `$SELECTION$` is alone on its line) wraps the whole
statements the selection touches, without their indentation; `parentheses`, `not` and the other
templates wrap exactly the selection and are listed only for a selection that holds code. The
wrapped lines lose the indentation of the first one's line and take that of the line
`$SELECTION$` is on, but for the lines that start inside a string or comment, which stay byte for
byte; the template's lines take the file's line breaks. Language modules declare surrounders next to their templates
(`TemplateSet.surrounders`, the JSON shape of `zen_workbench::templates::Surrounder`).
`view.surround.open` and `view.surroundWithTemplate.open` open the popup;
`get_window_state` lists it as `surround` or `surroundWithTemplate` while it is open.

## File types and ignored files

Settings → Editor → File Types as commands (ADR-0108); the guide's [Editor → File
Types](guide/09-settings.md#editor--file-types) says what the user sees. Types come from the built-in
modules, the loaded plug-ins (`extensions`, `names`, `hashbangs` and `contents` of `plugin.json`) and the
platform (`text`); what the user changes is kept per IDE in `filetypes.json` of the data folder.
Every change applies at once: open files take their new type for highlighting, code style, language
servers and icons, and the open projects are indexed again in the background (`search.index.status`).

| Command | Parameters | Result |
|---|---|---|
| `editor.fileTypes.list` | `{}` | `{ types, ignored, ignoredDefault }`: each type's `id`, `name` (in the interface's language), current `patterns` and `hashbangs`, `contents` (read-only, only for a type that has content markers: each as text, e.g. `<?xml`, `<!DOCTYPE (not <!DOCTYPE html)`) and `changed` (the user changed them); the ignored names and whether they are the defaults |
| `editor.fileTypes.typeOf` | `{ path }` | `{ id, name, byHashbang, byContent, overridden, highlighted, languageServer, ignored }`: the type of a file (its own first, see below) (a path relative to the project or absolute) by its best name pattern, else by the interpreter of its first line, else by its content markers (`byContent`: what the first 4 KiB start with, e.g. XML's `<?xml`), all read from the open text, else the file; `id` null for none; `ignored` when the file or a folder above it in the project is ignored |
| `editor.fileTypes.addPattern` | `{ type, pattern }` | `{ movedFrom, movedFromName }`: gives a type a name pattern (`*` any characters, `?` one, no `/`), taking it from the type that had it |
| `editor.fileTypes.removePattern` | `{ type, pattern }` | takes a pattern from a type; its files take the type of another pattern that names them, or none |
| `editor.fileTypes.addHashbang`, `editor.fileTypes.removeHashbang` | `{ type, pattern }` | the same for hashbangs: the interpreter's name (`bash`; `python` also names `python3.12`) |
| `editor.fileTypes.setIgnored` | `{ patterns }` | the list: replaces the ignored names; the tree, the index, Find in Files, Search Everywhere and Go to File leave them out |
| `editor.fileTypes.reset` | `{}` | the list: every type with its declared patterns and hashbangs, the default ignored names |

An unknown `type` and an empty pattern or one with `/` are errors. The best pattern wins: a whole
name over a pattern with wildcards over an extension (`*.yml`), then the one with more fixed
characters. The order that decides a file's type: the type the user gave the file itself (Override
File Type), its name patterns (the user's changes included), the hashbang of its first line, then
the content markers ([ADR-0135](adr/0135-file-types-by-content-and-xml-in-every-ide.md)). Content
markers are read from at most the first 4 KiB of a file (after a byte order mark and white space),
once per size and modification time, never from a file that is not a regular one or that has a NUL
byte in its head.

### Associate with File Type and Override File Type

ADR-0124; the guide's [Associate with File Type and Override File
Type](guide/05-project.md#associate-with-file-type-and-override-file-type) says what the user sees.
Two more types are listed: `detected` (File type auto-detected by file content: its files open as
text) and `native` (Files opened in associated applications: its files open in the system's
application, not in the editor). `file.document.open` on a file of no type whose first 8 KiB hold a
zero byte fails with the code `unknownFileType` and the data `{ path, patterns }` (the patterns the
dialog offers, `*.ext` first); the window shows the Associate with File Type dialog then. A text file
of no type opens as text.

| Command | Parameters | Result |
|---|---|---|
| `editor.fileTypes.associate` | `{ pattern, type, path? }` | `{ movedFrom, movedFromName, opened }`: gives the pattern to `type` (`detected`, `native` or any type's id) as `addPattern` does, then opens `path` as its type now says; `opened` is `editor`, `application` or null without `path` |
| `editor.fileTypes.override` | `{ paths, type }` | every override `[{ path, type, name }]`: Override File Type, the files themselves (not their pattern) get the type; `text` gives plain text, with no highlighting and no language server. Kept per IDE in `overridefiletypes.json`; folders, unknown types, `detected` and `native` are errors |
| `editor.fileTypes.revertOverride` | `{ paths }` | the overrides left: Revert File Type Override |
| `editor.fileTypes.overrides` | `{}` | every override |

`editor.fileTypes.typeOf` answers also `overridden` (the type is the file's own), `highlighted`
(the editor highlights the file) and `languageServer` (the id of the server that serves it, or
null). The action `view.associateFileType.open { path? }` (`run_action`; File → File Properties →
Associate with File Type…) opens the dialog for a file, the selected or the active one without
`path`; `get_window_state` lists it as `associateFileType`. The system's application is started with
`open` (macOS), `xdg-open` (Linux) or `explorer` (Windows), given the absolute path as its one
argument (no shell); with `ZEN_SYSTEM_OPENER=<program>` the IDE starts that program with the file's
path instead, so that end-to-end checks never open a real application. Only `file.document.open`
and `editor.fileTypes.associate { path }` start it; `file.document.open { path, editorOnly: true }`
(what reopening a project's tabs uses), navigation and Replace in Files refuse such a file instead,
and an executable file (or a link to one) is refused rather than started.
UTF-16 text (zero bytes on one half of the byte pairs, or a byte order mark) is not binary: it opens
decoded and read-only. A folder, pipe, socket or device is refused without being read. Renaming or
moving a file in the IDE (`file.fs.rename`, `project.file.cut` and `project.file.paste`) carries its
override along; deleting it in the IDE drops it.

## File templates

File templates ([ADR-0115](adr/0115-file-templates.md)) are commands of the project window (the
welcome window reaches the `file.templates.*` ones for the IDE's templates); the guide's
[File templates](guide/05-project.md#file-templates) says what the user sees. A template is
`{ name, extension, kind, fileName, text, reformat, liveTemplates }`, `kind` being `file`,
`include` or `code` and `fileName` the new file's name pattern without the extension (empty: the
name is asked for). `scheme` is `default` (the IDE's templates) or `project` (the project's
`.idea/fileTemplates`); left out, the scheme in use (`project` when that folder exists).

| Command | Parameters | Result |
|---|---|---|
| `file.templates.list` | `{ kind? }` | `{ scheme, templates }`: each template in use with `source` (`builtin`, `default`, `project`), `builtin`, `modified`, `origin?` (the module or plug-in), `default?` (the built-in one as declared), `asked` (the variables a new file asks for, `NAME` being the file name) and `problems?` (`{ include?, line, column, message }`, what cannot be rendered) |
| `file.templates.variables` | `{ directory?, name? }` | the predefined variables and those of modules and plug-ins: `{ name, description, value, origin? }`, the values for a new file `name` in `directory` |
| `file.templates.render` | `{ template, directory?, name?, variables? }` | `{ template, path, text, asked, missing, problems }`: the file the template would create, without creating it |
| `file.templates.create` | `{ template, directory?, name?, variables?, open? }` | creates the file in `directory` (the project folder by default; `name` may start with folders and end with the extension) and opens it unless `open: false`: `{ path, text, liveVariables?, reformatted }`. Refused, creating nothing, while a variable of `asked` has no value (`variables`, `{ "NAME": … }` may stand for `name`), when the template has problems, or when the file exists |
| `file.templates.set` | `{ template, scheme?, previous? }` | adds or changes a template (a built-in one becomes `modified`; one like the built-in one is no longer kept); `previous` renames one. Answers it as `list` does |
| `file.templates.remove` | `{ name, kind?, scheme? }` | removes a template the user made; a built-in one fails |
| `file.templates.reset` | `{ name?, scheme? }` | `{ reset }`: how many changed built-in templates are back as declared |
| `file.convert.toLanguage` | `{ path?, to, keep? }` | converts a file (the active one by default) into the language of the extension `to` (`kt` for Java into Kotlin) with the plug-in that converts into it: writes the new file beside it, opens it and moves the original to the Trash unless `keep`: `{ path, removed, problems }`, `problems` with `line` and `column` of the original from 1. Refused when the new file exists or no plug-in converts |
| `editor.convert.text` | `{ from, to, text }` | `{ text, problems }`: the code converted from the language of `from` into that of `to` (a whole file, members or statements); nothing in the project changes |
| `file.templates.saveFileAs` | `{ path?, name?, scheme?, replace? }` | File → Save File as Template…: a template of the file's text (the active file's by default, unsaved changes included), named after the file; fails when the name is taken unless `replace` |

The window actions `view.fileTemplates.new { template?, directory? }` (New File from Template…: the
dialog with the file name and a field per asked variable) and `view.fileTemplates.saveAs` (the
template editor filled in from the file in front) open the dialogs `newFromTemplate` and
`saveFileAsTemplate`.

## Context actions

`editor.intentions.list { path?, offset?, waitMs? }` asks the file's language server for the
context actions (⌥Enter) at a character offset, else at the primary caret or its selection:
`{ pending, actions, error }`, each action with `id`, `title`, `kind` (`quickfix`,
`refactor.extract`, `source.organizeImports`..., none for a bare command), `quickFix`, `preferred`
and `disabled` (the reason, when it cannot be applied), quick fixes first. The diagnostics touching
the place go with the request; a caret on a line with a problem asks about that problem. Asking
again at the same place and text reuses the answer; call with `waitMs` while `pending`.

`editor.intentions.apply { id, waitMs? }` applies one by its `id` from the last listing: it is
resolved first (`codeAction/resolve`) when it has neither an edit nor a command, its edit is made in
every file or none (open files unsaved, ADR-0044), and its command runs through
`workspace/executeCommand`, the edits the server sends meanwhile (`workspace/applyEdit`) applied
the same way. The whole action is one ⌘Z in each file, and when any part of it fails everything it
changed is undone. The answer is `{ title, files, command }`. It fails, changing nothing, when the file
changed since the listing (list again), the id is unknown, the action is disabled, the file is
read-only, an edit would write another IDE's project settings (`.idea/`) or a file outside the
project (symbolic links resolved). Edits a server sends
when no action runs are refused (ADR-0058). `view.intentions.open` (⌥⏎) opens the popup over the
same commands; `get_window_state` lists it as `intentions` while it is open.

With text selected (and no `offset`), the list ends with `scratch.fromSelection`, Create new scratch
file from selection, also when no server runs (the server's error then comes in `error` beside it);
applying it is `file.scratch.fromSelection` (ADR-0122). Scratch files may be edited by actions.

## Generate

`editor.generate.list { path?, offset?, waitMs? }` lists what Code → Generate… (⌘N) can write at a
character offset of an open file, else at the primary caret of the active one: `{ pending,
generators, error }`, each generator with `id`, `title`, `kind` (the server's code action kind, none
for the language's own) and `chooser`. The language module's or plug-in's own generators come first
(Go: `Constructor` inside a struct type), then the server's code actions of kind `source.*` asked
with `only: ["source"]`, without organizing imports, fixing all and the kinds its declaration does
not name (`generate.kinds`, ADR-0216). Call with `waitMs` while `pending`; the ids hold until the
next listing or change of the file. A generator that `view.overrideMethods.open` (⌃O) or
`view.implementMethods.open` (⌃I) opens directly says so with `action`: `override` or `implement`.

`editor.generate.members { id, waitMs? }` gives the member chooser of a generator with `chooser`:
`{ pending, chooser: { title, members, options, allowEmpty } }`, each member with `id`, `name`,
`detail`, `group` (the type it belongs to) and `selected`, each option with `id`, `label` and
`value`. For a server's generator declared with `generate.prompts` it sends the declared request
with the action's context; call again with `waitMs` while `pending`.

`editor.generate.apply { id, members?, options?, waitMs? }` writes the code with the members given
(the `selected` ones when absent) and the options (the chooser's values for those absent), as one
undo step, and answers `{ title, files }`; a plain server action is applied as
`editor.intentions.apply` applies it. It fails, changing nothing, for an unknown or stale id, an
unknown member, no member for a generator that needs one, or a read-only file; the listing is
dropped after every attempt, so list again. `view.generate.open` (⌘N in the editor) opens the
popup; `get_window_state` lists it as `generate`, and the member chooser as `memberChooser`.
`view.overrideMethods.open` and `view.implementMethods.open` open the member chooser of the
generator with that `action` without the popup, or a notice when there is none or nothing to choose.

## Unwrap/Remove

`editor.unwrap.list { offset? }` lists the entries of Code → Unwrap/Remove… (⇧⌘⌦) at a character
offset of the active file, else at the primary caret, innermost first: `[{ index, kind, title,
construct, start, end, replacement, kept }]`. `kind` is `unwrap` (the construct goes, its body
stays, re-indented to where the construct was) or `remove` (an `else` branch or a `case` goes with
its body); `title` is the popup's text (`Unwrap 'if…'`, `Remove 'else…'`, `Unwrap braces`);
`construct` names it (`if…`, `else…`, `for…`, `while…`, `case…`, `default…`, `func…`, `f(…)`, none for
braces); `start`..`end` is the character range the edit replaces with `replacement`, and `kept` the
`[start, end]` ranges that stay, which the popup highlights. The constructs are found from the text
without a parser (ADR-0099): `if`/`else` chains, loops, `switch` and `select` cases, function
literals, bare braces and calls with one argument; brackets in strings and comments (from the file's
highlighter) do not count.

`editor.unwrap.apply { index, offset? }` applies the entry `index` of the list at the same place as
one undo step and puts the caret where its code went; it fails, changing nothing, when there is no
such entry or the file is read-only. `view.unwrap.open` (⇧⌘⌦) is the window action: with one entry
it applies it at once, with several it opens the popup (`get_window_state` lists it as `unwrap`);
its result is `{ entries, applied, popup }`.

## Extract and Inline

`refactor.extract.variable`, `refactor.extract.constant`, `refactor.extract.field`,
`refactor.extract.parameter`, `refactor.extract.method` and `refactor.inline.apply`
`{ path?, start?, end?, variant?, name?, waitMs? }` ask the file's language server for its code
actions of kinds `refactor.extract*` and `refactor.introduce*` (`refactor.inline*` for Inline) at
`start`..`end` (the primary selection by default) and apply the one that fits: by kind
(`refactor.extract.variable`, `.constant`, `.field`, `.function`, `.method`,
`refactor.introduce.parameter`) and, for servers that send a bare `refactor.extract` or a
TypeScript `refactor.extract.constant`, by title (ADR-0219). The answer is
`{ pending, start, end, expressions, variants, title, files, path, name, occurrences, suggestions }`:
with an empty range and several expressions around the caret, `expressions` lists them and nothing
changes (call again with one's `start` and `end`); with several fitting actions, `variants` lists
their titles (call again with `variant`); while the server has not answered, `pending` (call again
with the returned `start` and `end`). After the change, which is one undo step, `name` is the new
name, found from a snippet placeholder the server sent or as the identifier the change brought in,
and `occurrences` every place of it in `path`; `name` in the parameters renames them at once, in
the same undo step. The window actions `view.extract.variable` (⌥⌘V), `view.extract.constant`
(⌥⌘C), `view.extract.field` (⌥⌘F), `view.extract.parameter` (⌥⌘P), `view.extract.method` (⌥⌘M) and
`view.inline.open` (⌥⌘N) show the choices as popups and the name in place; `get_window_state` lists
them as `extractExpressions`, `extractVariants` and `inPlaceName`.

## Safe Delete and the refactoring dialog

`refactor.safeDelete.find { path?, offset? | line?+column?, paths?, waitMs? }` finds what keeps the
declaration whose name is at the place (the caret by default), or the files and folders `paths`,
from being deleted safely: `textDocument/documentSymbol` names the declaration (every top-level one
of a file), `textDocument/references` without the declaration finds its usages, and those inside
the declaration or the deleted files are left out. The answer is
`{ pending, name, usages, total, deletes, operations, error }`: `usages` by file, `deletes` the text
the deletion removes (the declaration with the attributes, annotations and doc comment above it),
`operations` the files moved to the Trash. `refactor.safeDelete.apply` (same parameters and
`force`) deletes when there are no usages, or with `force: true`; a declaration's deletion is one
undo step, files go through `file.fs.delete` with the servers' `willDeleteFiles`.
`refactor.safeDelete.cancel` forgets the search.

The shared refactoring dialog runs refactorings that providers declare (ADR-0222):
`refactor.dialog.list` names them (`safeDelete`, `rename`, `javaMove`, `javaChangeSignature`,
`javaPullUp`, `javaPushDown`, `javaExtractSuperclass`, `javaExtractInterface`,
`javaEncapsulateFields`, `javaIntroduceParameterObject`), each `{ id, title, action? }` where
`action` (`move`, `changeSignature`, `pullMembersUp`, `pushMembersDown`, `extractSuperclass`,
`extractInterface`, `encapsulateFields`, `introduceParameterObject`) is the window action it answers;
`refactor.dialog.form { provider, context, waitMs? }` gives
`{ pending, title, message, fields, immediate, conflictsTitle, anywayLabel, viewLabel, error }`,
each field `{ id, label, kind: text|checkbox|choice|table, value, options, columns, newRow, fixed }` (a
table's value is a list of rows, objects with a value by column id, which may carry other keys the
provider reads back; each column `{ id, label, kind: text|checkbox|label }`, a `checkbox` cell a
boolean, none when the row has no boolean there; `newRow` is the row **Add** inserts, and a
`fixed` table has no Add, Remove or Move buttons);
`refactor.dialog.preview { provider, context, values, waitMs? }` gives
`{ pending, files, operations, usageCount, conflicts, conflictsMessage, error }`, each conflict
`{ message, place?, relative? }`; `refactor.dialog.apply` (same, with `force`) refuses while there
are conflicts unless `force: true`; `refactor.dialog.cancel { provider }` forgets its work. The
`context` is the refactoring's own: `{ path, offset }` (or `line` and `column`) for a declaration,
`{ paths }` for Safe Delete of files. The window actions `view.safeDelete.open` (⌘⌦: the caret's
declaration in the editor, the selection in the Project tool window, or `paths`) and
`view.refactoring.open { provider, context }` open the dialog; `view.project.move` (F6: `path`,
`paths`, `context`, the Project tool window's selection or the editor's file; a scratch file keeps
its own Move dialog), `view.changeSignature.open` (⌘F6: `context` or the caret),
`view.pullMembersUp.open`, `view.pushMembersDown.open`, `view.encapsulateFields.open`,
`view.extract.parameterObject`, `view.extract.interface` and `view.extract.superclass` (`context`
or the caret) open it with the
refactoring whose `action` answers that place, and answer `{ provider, context }`; `get_window_state` lists it as
`refactoringLoading`, `refactoringDialog` (the fields) or `refactoringConflicts` (the conflict list).

### Java: Move and Change Signature

`javaMove` takes `{ paths }` (`.java` files, or one package folder) or `{ path }` (the class of that
file). Its form asks JDT LS `java/getMoveDestinations` (`moveKind: moveResource`) and gives one
choice field, `destination`, whose values are the packages' folder URIs; its preview sends
`java/move` with the server's own destination and `updateReferences: true` for classes, and asks
`textDocument/references` of every top-level class without `public` to list the uses left behind
in the old package as conflicts. A package moves as a folder rename: the preview asks the server's
`workspace/willRenameFiles` for the new folder and adds the rename; the apply sends
`workspace/didRenameFiles` (ADR-0229).

`javaChangeSignature` takes `{ path, offset }` (or `line` and `column`; the caret by default) on a
method's name or a call (then `textDocument/definition` finds the method). Its form comes from
`java/getChangeSignatureInfo`: `visibility` (choice: `public`, `protected`, `` for package-private,
`private`), `returnType`, `name`, `parameters` (table of `type`, `name`, `defaultValue`, each row
with the `originalIndex` it had, -1 for a new one) and `delegate` (check box). Its preview sends
`java/getRefactorEdit` with `command: changeSignature` and splits the server's edit into its
changed lines; a new parameter needs a default value, names must be unique identifiers, and a
removed parameter still used in the body is a conflict.

### Java: class refactorings

These providers take `{ path, offset }` (or `line` and `column`; the caret by default) in a class,
on a member or in a method (ADR-0232). Their member tables are `fixed` with the columns `selected`
(check box), `member` (label) and, where it applies, `abstract` (check box, only on methods with a
body); each row carries the member's `signature` (`name` for a field, `name(types)` for a method),
which is what the preview reads back. Edits are planned from the Java scanner unless said
otherwise; they are applied like the server's and are one undo step.

- `javaPullUp`: the form asks `textDocument/prepareTypeHierarchy` and `typeHierarchy/supertypes`;
  `destination` is a choice of the project's supertypes (file URIs), `members` the table with
  `abstract` ("Make abstract"). Conflicts: a moved member uses one that stays, the destination
  has the member, the destination becomes abstract.
- `javaPushDown`: `typeHierarchy/subtypes` finds the direct subclasses; `members` with `abstract`
  ("Keep abstract"). The preview asks `textDocument/references` of each member that goes and lists
  uses outside the subclasses, and private members the moved code uses.
- `javaExtractSuperclass`: `name` and `members` with `abstract`; creates `<name>.java` beside the
  class, which then extends it (it extends the old superclass). No server request.
- `javaExtractInterface`: the form asks `java/checkExtractInterfaceStatus`; `name`, `destination`
  (the server's packages) and `members` (rows carry the server's `handle`); the preview sends
  `java/getRefactorEdit` with `command: extractInterface` and drops the server's generated
  `(non-Javadoc)` comments.
- `javaEncapsulateFields`: `fields` (columns `selected`, `field`, editable `getter` and `setter`;
  rows carry `name`), `fieldVisibility` and `accessorVisibility` (choices: `public`, `protected`,
  `` for package-private, `private`). The preview asks `textDocument/references` and rewrites uses
  outside the class; uses it cannot rewrite are conflicts.
- `javaIntroduceParameterObject`: `className`, `kind` (`class` or `record`), `parameterName`, and
  `parameters` (columns `selected`, `parameter`; rows carry `name`). The preview asks
  `textDocument/references` (the calls) and `textDocument/implementation` (overriding methods are
  refused), creates `<className>.java` and rewrites the method and its calls.

## Scratch files and buffers

Scratch files live in `<data folder>/scratches`, buffers in `<data folder>/buffers`, shared by every
project (ADR-0122, guide: [Scratch files](guide/25-scratches.md)).
`file.scratch.languages { query? }` lists the languages `[{ id, name, extension }]`, recent first;
`file.scratch.new { language, text? }` creates `scratch.<ext>`, `scratch_1.<ext>`... and opens it,
without `text` with what the language starts a scratch with (Go: a `main`, the caret inside);
`file.scratch.fromSelection` makes one from the active editor's selection;
`file.scratch.newBuffer { clear? }` opens `buffer1.txt`...`buffer5.txt` in turn and answers
`{ path, reused, cleared, needsAnswer }`: a reused buffer with text opens only when called again
with `clear`; `file.scratch.list` gives `{ folder, buffersFolder, scratches, buffers }`;
`file.scratch.changeLanguage { path, language }` and `file.scratch.move { path, directory }` (into a
project folder) answer the new path; `file.scratch.run { path, waitMs? }` runs a scratch file on
its own through its language's provider (ADR-0212) and answers `run.process.start`'s `{ runId,
summary? }`; for a scratch file `run.targets.list` and `run.context.get` answer only that run. A Kotlin scratch runs as kind `kotlin.file` with the installed
`kotlinc` (ADR-0230). For an agent, a file hidden from agents is refused (code `redacted`) by these and by
`run.process.start`'s `context`. `file.fs.rename` and `file.fs.delete` take scratch files, and
closing an empty scratch file (`file.document.close`) deletes it unless it has text saved on disk or
another window shows it. In the window, `view.scratch.new`
(⇧⌘N), `view.scratch.newBuffer`, `view.scratch.show`, `view.project.changeLanguage { path }` and
`view.project.move { path }` (F6) open the popups and dialogs, which `get_window_state` lists as
`scratchLanguages`, `scratchFiles`, `clearBuffer` and `moveScratch`; the setting
`projectViewScratches` shows the Project tool window's Scratches and Consoles. `runConfirmRerun` (on) makes the window ask before rerunning a running
configuration; `runTemporaryLimit` (1 to 100, 5) is how many temporary run configurations are kept.

## Tool window layout

Each tool window has a side, an order on its stripe, a size and whether it is shown, remembered per
project in `workspace.json` (guide: [Tool windows](guide/22-tool-windows.md)). The ids are
`project`, `commit`, `structure`, `find`, `debug`, `run`, `terminal`, `problems`, `git`, `services`, `todo` and
`notifications`; the
sides `leftTop`, `leftBottom`, `bottomLeft`, `bottomRight`, `rightTop` and `rightBottom`.
`get_window_state` gives `toolWindows: { windows: [{ id, side, order, size, visible }], active }`,
where `active` is the tool window shown or clicked last, the one the actions below act on when no
`id` is given.

| Action | Parameters | Does |
|---|---|---|
| `view.toolWindow.move` | `id?`, `side` (one of the sides, or `left`, `right`, `bottom`), `index?` (place among that side's tool windows, the end by default) | Moves a tool window; a shown one shows in its new place. Answers `{ id, side }` |
| `view.toolWindow.hide` (⇧⎋) | `id?` | Hides a tool window |
| `view.toolWindow.hideAll` (⇧⌘F12) | none | Hide All Tool Windows: hides those shown; with none shown, shows again those it hid |
| `view.toolWindow.resize/<left\|right\|up\|down>` (⌃⇧←→↑↓) | `id?`, `by?` (points, 20 by default) | Runs `view.toolWindow.resize { direction }`: Stretch to Left, Right, Top or Bottom |
| `view.layout.restoreDefault` (⇧F12) | none | Restore Default Layout: every tool window at its default side, order and size, only Project shown |

`view.toolWindow.project` and the other tool window actions show or hide a tool window wherever it
is.

## Menus and toolbars

The `ui.customization.*` commands edit what **Settings → Appearance & Behavior → Menus and Toolbars**
edits, and each saves at once. `ui.customization.list` gives the groups with their items and the
built-in items taken out (`missing`); `ui.customization.add` (`group`, `item`, `after`),
`.remove` and `.move` (`group`, `index`) change one item, `.restore` brings back one group or, with
no `group`, all of them, and `.setIcon` gives an action a built-in icon. An item is an action or
command id, `-`, or a `slot:` id of a built-in item. `set_settings` with `customization` replaces a
whole list.

## Window actions

Most actions are commands of the window's session, which `dispatch_command` runs. The keymap also
has actions of the window itself, named `view.*`: they show a tool window (`view.toolWindow.problems`,
⌘6), a popup (`view.searchEverywhere.open`, `view.projects.popup`, `view.switcher.open`, `view.recentFiles.open`, `view.recentLocations.open`, `view.fileStructure.open`, `view.customFolding.open`, `view.selectIn.open`, `view.pasteFromHistory.open`, `view.runAnything.open`, `view.runConfigurations.choose`, `view.bookmark.mnemonic`, `view.bookmark.goToMnemonic`, `view.bookmark.show`) or a dialog
(`view.settings.open`, `view.findInFiles.open`, `view.vcs.push`). `get_keymap` and `find_action`
list them with the others.

- **`run_action { action, params?, project? }`** runs one, exactly as its shortcut, its menu item or
  Find Action runs it (`IdeView::perform_action`, `ide/agent.rs`). `action` is a keymap action
  (`view.settings.open`, `editor.caret.move/wordLeft`, which runs `editor.caret.move` with the
  binding's parameters) or a command id; `params` replaces the binding's parameters. The answer is
  `{ action, result, window, changed }`: the command's result (`null` for a `view.*` action, except what `view.settings.open { query }` found), what
  the window shows afterwards, as `get_window_state` returns it, and whether that changed. Running a
  tool window's or popup's action again hides it, as its shortcut does.
- **`dispatch_command`** with an id that is not a command but a keymap action does the same, so the
  `command` of every `find_action` and `get_keymap` entry runs there too.
- **`get_window_state { project? }`** returns `{ left, bottom, right, toolWindows, popups, editor, workspace, preview, splits, appearance, fullScreen, ideZoom, projectView, todo }`:
  the tool window shown on the left, at the bottom and on the right (the first half's before the
  second's, or `null`), the layout of every tool window (see [Tool window layout](#tool-window-layout)), the popups and dialogs open
  (`settings:<page>` such as `settings:mcp` or `settings:keymap`, `searchEverywhere:<all|classes|files|symbols|actions>`,
  `findInFiles`, `replaceInFiles`, `tooManyResults`, `projects`, `switcher`, `recentFiles`, `recentLocations`, `fileStructure`, `customFolding`, `selectIn`, `pasteFromHistory`, `navigationBar`, `navigationBarPopup`, `bookmarkMnemonic`, `goToMnemonic`, `bookmarkReplace`, `bookmarkRename`, `bookmarkList`, `bookmarkListName`, `bookmarkListDelete`, `lineBookmarks`, `runConfigurations`, `runAnything`, `chooseRunConfiguration`, `chooseDebugConfiguration`, `rename`, `intentions`, `unwrap`, `showUsages`, `gotoLine`,
  `rollback`, `tipOfTheDay`, `myProductivity`, `newEntry`, `newFromTemplate`, `saveFileAsTemplate`, `delete`, `confirmExit`, `zoomIde`, `bookmarkOpenTabs`, `localHistory`, `localHistoryLabel`, `branches`, `newBranch`, `checkoutRevision`, `updateProject`,
  `push`; the Git workspace's `commitMenu`, `refMenu`, `stashMenu`, `createTag`, `checkoutCommit`, `resetHard`,
  `renameBranch`, `setUpstream`, `deleteRemoteBranch`, `dropStash`, `renameStash`, `gitConflicts`, and the changelist and shelf dialogs `newChangelist`, `editChangelist`, `moveToChangelist`, `shelve`, `unshelve`, `renameShelf` and `deleteShelf`, and the patch dialogs `createPatch` and `applyPatch`), the file in the editor (its path, `diff` for a diff tab, `gitWorkspace` for the Git
  workspace tab, `workspace` while an expanded workspace shows a tab that is not a file, or `null`), the
  workspace in front (`workspace: { id, expanded, activeTab: { id, kind, title, path? } }`, the expanded
  one or else the one expanded last, `null` without one; its view's open popups are among `popups`; see
  [Database](#database)), the preview of that file when it has one (`preview: { kind, mode, orientation, scope }`,
  e.g. `{ "kind": "svg", "mode": "editorAndPreview", "orientation": "vertical", "scope": "default" }`, see [Text with a preview](#text-with-a-preview)), the split editors (`splits`: the groups' tree as `editor.split.list`
  returns it, or `null` when the editor is not split), the View → Appearance mode (`presentation`,
  `distractionFree`, `zen` or `null`; `view.presentationMode.toggle`, `view.distractionFreeMode.toggle`
  and `view.zenMode.toggle` switch them) and whether the window is full screen or has been asked to be
  (`view.fullScreen.toggle`), and the IDE zoom in percent (`ideZoom`; see [Settings](#settings)). While a mode is on, `left` and `bottom` are what it shows; the tool windows
  it hid come back when it ends. `projectView` holds the Project tool window's options:
  `{ alwaysSelectOpened }`. Commands without a `group` act on the split that has the keyboard (an
  expanded workspace's group while it is in front):
  `file.document.close { index? }` and `editor.tab.select { index }` count tabs in that split, which,
  with the editor not split, are the open files in order, as before. Closing tabs saves their unsaved
  changes first, as Save All does: `file.document.close { group?, index? }`,
  `file.document.closeOthers { group?, index? }` (Close Other Tabs, sparing pinned tabs),
  `file.document.closeAll { group? }`, `file.document.closeUnmodified { group? }` (only the tabs
  without unsaved changes), `file.document.closeAllButPinned { group? }`,
  `file.document.closeLeft { group?, index? }`, `file.document.closeRight { group?, index? }` (both
  sparing pinned tabs) and `file.document.closeReadOnly { group? }` answer
  `{ closed, saved }` (`file.document.close` without `group` and `index` while a workspace shows a tab
  that is not a file closes that tab and answers its id as `tab`), the files whose tabs closed and those saved; a file that cannot be saved keeps
  its tab and changes, the others close, and the command fails naming it. `close_project` and
  `exit_ide` save every changed file the same way and fail, leaving the window and the IDE open,
  when one cannot be saved. No save writes over a file that changed on disk since the IDE read it
  (ADR-0238): `file.document.save`, `saveAll`, the closes, `save: true` of `editor.text.*` and the
  commands that save before Git work fail naming the file, the changes stay unsaved and the document
  is a conflict until `file.document.reloadFromDisk` or `file.document.keepMine`.
  `editor.tab.move { group?, index?, to }` moves a tab along its row as dragging it does, never an
  unpinned tab before a pinned one (refused while tabs are sorted alphabetically);
  `project.tab.move { to }` moves the project's tab in the main window's row of project tabs;
  `editor.tab.pin { group?, index?, pinned? }` pins or unpins a tab (toggles without `pinned`) and
  answers its group, whose `pinned` lists the pinned files, kept at the start of `tabs`;
  `file.document.reopenClosed` reopens the tab closed last (`{ path, group, index, pinned }`, the
  latest 30 kept per project); `editor.tab.next` and `editor.tab.previous` cycle the tabs of the
  split with the keyboard and answer the file shown; `editor.split.moveToOpposite { group?, index? }`
  moves a tab to the next split.
  `file.document.open { path, preview: true }` shows a file in the group's reused preview tab while
  `editorTabs.preview` is on (`set_settings`), an ordinary tab otherwise; the group's `preview` names
  that file; `editor.tab.keepOpen { group?, index? }` and opening the file without `preview` make it
  permanent, as an edit does. `editorTabs` in `get_settings` and `set_settings` holds everything on
  the Editor Tabs page and Advanced Settings → Editor Tabs; `run_action` with
  `view.settings.open/editorTabs` opens that page.
  **`file.document.autosave`** saves every modified file as the IDE's autosave does (ADR-0110),
  through the same path, and answers `{ saved, skipped, failed }`: `skipped` lists `{ path, reason }`
  with `reason` `readOnly` (opened read-only, or no write permission), `conflict` (the file changed
  on disk; the document is marked as a conflict until `file.document.reloadFromDisk` or
  `file.document.keepMine`) or `missing` (not on disk); `failed` lists `{ path, error }`, each also
  a notification the first time the file fails (a formatter's failure too, until the file saves
  cleanly). The IDE runs it by itself after `autosaveIdleSeconds` without input and without a
  change to the modified files (`autosaveOnIdle`), and when a window loses the keyboard to another
  application or a terminal session gets it (`autosaveOnFrameDeactivation`); a headless IDE has no
  input, so it saves an agent's edits `autosaveIdleSeconds` after the last one.
- **The Project tool window's header.** `run_action` with `view.project.selectOpened` is **Select
  Opened File**: it selects the editor's file in the tree, unfolding the folders on the way, shows the
  tool window and answers `{ selected }`; with no file in the editor, or one outside the project and
  its libraries, it is refused. `view.project.alwaysSelectOpened { on? }` sets **Always Select Opened
  File** (switches it without `on`) and answers `{ alwaysSelectOpened }`; while it is on, the tree
  follows the editor's tab and the header hides Select Opened File. It is kept per project in
  `workspace.json` (`projectView.alwaysSelectOpened`) and restored when the project opens.
- **`press_keys { keys?, text?, project? }`** puts key presses (written as on macOS: `⎋`, `⏎`, `↓`,
  `⇧⌘A`, `F2`), then typed text, into the window's input, as the keyboard would; the system keyboard
  is never used. The keys go into the next drawn frame; text given with keys goes into the frame
  after, so that it reaches what the keys opened (the field of Find Action after `⇧⌘A`), as when a
  person types after a shortcut. It answers once all of it is in, with `{ window, changed }`. `⎋`
  closes the dialog or popup that has the keyboard; `{ "keys": ["⇧⌘A"], "text": "problems" }`, then
  `{ "keys": ["⏎"] }` choose an action in Find Action as a person does. The numeric keypad's `*` is
  `NumPad*`, and a shortcut of two keystrokes is pressed as written in the keymap
  (`{ "keys": ["⌘NumPad*, 2"] }`) or as two keys in a row.

**Find Action lists every registered command.** `find_action` (and the ⇧⌘A popup) lists the
keymap's actions and every other command by its title (`command-<id>` in
`zen-ui/locales/*/commands.ftl`), each entry with `action`, `name`, `group`, `shortcut`, `command`
(the id `dispatch_command` takes), `params` and `available`. Choosing a command, or `run_action`
with its id and no `params`, runs:

  What the keys reach runs as the agent: a command, button or dialog refuses it as it refuses the
  agent's own call, and a change in Settings passes what `set_settings` refuses.
- the command with `{}` when it needs no parameters (`toolchain.gopls.install`,
  `project.model.refresh`);
- otherwise the window action that asks for them, named in the entry's `opens`
  (`search_everywhere::command_ui`): `ui.theme.select` and `ui.theme.import` open Settings →
  Appearance, `ui.keymap.*` Settings → Keymap, `toolchain.go.choose` Settings → Go (a built-in
  module names the commands its settings page is for, `SettingsPage::commands`),
  `vcs.branch.checkout` the Branches popup, `run.configuration.*` Run/Debug Configurations; a few
  commands whose result people see in a tool window or on a Settings page open it
  (`problems.project.list` → Problems; `language.servers.status`, `lsp.servers.list`,
  `lsp.server.install`, `lsp.server.restart`, `lsp.server.configure` and `lsp.server.undismiss` →
  Settings → Languages & Frameworks → Language Servers). Agents get such a command's own result
  with `dispatch_command`;
- commands with required parameters and nowhere to give them (`editor.edit.type`) are not listed:
  agents run them with `dispatch_command` and their parameters. Nor are the window actions that need
  such parameters (`view.diff.setText`, a diff pane's text): agents run them with `run_action`. Commands whose entry is a keymap
  action already (`vcs.commit.create` is **Commit…**) are not listed twice;
- The three modes of a text with a preview are actions of their own: `editor.preview.editorOnly`,
  `editor.preview.editorAndPreview` and `editor.preview.previewOnly`.
- `editor.image.zoom` is listed once for each of its steps, as the image toolbar names them:
  `editor.image.zoom/in`, `/out`, `/actual` and `/fit`, each with its `params`
  (`{"action": "in"}`); `run_action` takes these actions too.

`run_action` with `params` runs the command itself with them. A test
(`find_action_finds_every_registered_command_and_runs_it`) checks that every registered command has
a name in English and Russian.

An action runs at once, drawn frame or not; what it opens is drawn in the window's next frame, so a
`ZEN_UI_SCREENSHOT` taken afterwards shows it. Keys and text that `press_keys` gives right after an
action opened a popup or a dialog go into it, in their order, even in the frame the popup is first
drawn in and while another popup is closing: that frame's keys wait for the next frame, when the
popup's field has the keyboard, and the answer comes after it, one frame later than usual
(`typing_at_once_after_search_everywhere_closes_goes_into_the_popup_opened_next`,
`keys_typed_in_the_frame_a_popup_opens_reach_it_in_their_order`).
A project in a tab of the main window that is not the
front tab comes to the front first, since only that tab is drawn. What needs a person is refused with
a message: **Open…** (`view.project.open`) asks for a folder in a system dialog, so agents call
`open_project`; **Rename** (`view.rename.open`) and **Go to Line** (`view.gotoLine.open`) need a
text file in front in the editor: with a diff tab, an image or no file in front they are refused,
saying what is there instead, and never open unseen. A diff tab gives way as soon as a document is
brought to the front (`file.document.open`, even of the file the diff is over, or
`editor.tab.select`); Go to Line or Rename left open over a file close when a diff or an image comes
in front of it.
A command's failure comes back as the error and also shows in the window as it would for a person.

`every_keymap_action_runs_through_the_automation_layer` in `zen-ui` (`ide/app.rs`) runs each action
of the keymap through `run_action` and `dispatch_command` in a window with a Go file open, and
fails on any "unknown command"; `agents_open_and_close_tool_windows_and_dialogs` opens and closes a
tool window and the Settings dialog, and chooses an action in Find Action with `press_keys` (keys
and text in one call too);
in Settings, `press_keys` with `PageDown`, `PageUp`, `Home` or `End` scrolls the page as it does
for a person, unless a field of the page has the focus (`page_keys_scroll_the_page`);
`text_actions_are_refused_without_a_text_file_and_a_file_comes_back_over_a_diff` checks the
refusals and the diff tab giving way.

## Running and testing

Run configurations, runs and their test results are commands of the project window, dispatched with
`dispatch_command` ([ADR-0048](adr/0048-run-configurations-and-the-run-service.md); the file is
described in [run-configurations.md](run-configurations.md)):

| Command | Parameters | Result |
|---|---|---|
| `run.configuration.list` | `{}` | `{ configurations, selected, kinds }`: the permanent configurations of the file, then the ones adopted from a JetBrains IDE's `.idea`/`.run` files (read-only, with `adoptedFrom`), then the temporary ones of the window, the selected id, and each kind with its provider and option fields |
| `run.configuration.create` | `{ configuration, select?, template? }` | The configuration with its new id; what it leaves out comes from its kind's template unless `template: false`. A compound (`kind: "compound"`, `options.configurations`: ids) that contains itself, directly or through another compound, is refused |
| `run.configuration.new` | `{ kind, name? }` | A configuration of the kind as the dialog's + makes it, from the kind's template, without an id and not saved |
| `run.template.list` | `{}` | `{ templates }`: each kind (but `compound`) with its `provider`, `stored` and the `template` `{ kind, common, options }` new and temporary configurations of it start from (its defaults when not `stored`) |
| `run.template.update` | `{ template }` | The template, saved under `templates` in `.zen/runConfigurations.json`; existing configurations do not change |
| `run.template.reset` | `{ kind }` | The kind's defaults; the project's template of it is removed |
| `run.configuration.update` | `{ configuration }` | The configuration, replacing the one with its id; refused for an adopted one |
| `run.configuration.delete` | `{ id }` | Nothing; refused for a configuration adopted from a JetBrains IDE |
| `run.configuration.saveAdopted` | `{ id }` | The copy of an adopted configuration (id `jb:…`, with `adoptedFrom`) saved into `.zen/runConfigurations.json`, with its new id; it takes the adopted one's place |
| `run.configuration.select` | `{ id? }` | Nothing; Run (⌃R) starts the selected one. The selection is kept with the window, not in the file |
| `run.context.get` | `{ path?, offset?, line? }` | `{ configuration?, existing? }`: what runs at a character offset, or at a one-based line as `run.targets.list` and failed tests give it (the active editor's caret by default), and the saved configuration it matches; `path` is absolute or relative to the project |
| `run.targets.list` | `{ path, full? }` (absolute or relative to the project) | `{ targets }`: what a file runs by one-based line, as the gutter's run icons, each `{ provider, line, kind, name, package? }` (`package` the package its test results name); `run.process.start { context: { path, line } }` runs one; `full: true` adds the `configuration` that `run.context.get` gives at its line, which the gutter runs |
| `run.configuration.resolve` | `{ configurationId? \| configuration?, prompt?, filePrompt? }` | `{ programArguments, toolArguments, workingDirectory, env }`: what the configuration would launch with now, its `.env` files read, `$NAME$` references and macros expanded; fails as `run.process.start` would |
| `run.environment.system` | `{}` | `{ variables }`: the environment every run inherits, by name |
| `run.process.start` | `{ configurationId? \| context? \| configuration?, waitMs?, prompt?, filePrompt?, confirmRerun? }` | `{ runId, summary?, children?, errors? }`; with `{}` the selected configuration runs. A compound starts each of its configurations (inner compounds' ones too, each once) in a run of its own: `children` holds their `{ runId, summary? }`, `runId` is the first's, and `errors` says why any did not start; with `waitMs` it waits for all of them. Modified documents are saved first. A configuration that runs already is stopped and started again (the new run's `replaces` names the stopped one), unless its `common.allowMultipleInstances` is on: then another instance starts. With `confirmRerun: true` that case fails with the code `rerunConfirmation` instead. `prompt` and `filePrompt` answer `$Prompt$` and `$FilePrompt$`; without them a configuration using them fails with the code `promptNeeded`. With `waitMs` (at most 600 000, ten minutes) the answer comes once the run has ended or the time is up, with `summary` as `run.tests.get` gives it; the IDE goes on meanwhile, and the MCP server waits that much longer for it |
| `run.process.stop` | `{ runId? \| configurationId? }` | `{ stopped?, runs? }`; without `runId` the run started last that still runs; with `configurationId` every running run of that configuration and, for a compound, every run it started (`runs`). The program and its process group get an interrupt (SIGINT; `taskkill /T` on Windows) and are killed 2 s later, or at once by a second stop; the run ends `stopped` when they have ended |
| `run.process.list` | `{}` | `{ runs }`: each run's `runId`, `name` (`name (2)` for a second instance), `configurationId`, `kind`, `state`, `exitCode`, `instance`, `replaces?`, `compound?` (the compound configuration that started it), `activateToolWindow`, `focusToolWindow`; the window keeps the newest 20 runs that have ended and every running one |
| `run.services.list` | `{}` | `{ kinds, hidden, available }`: what the Services tool window (⌘8) shows, by kind in the order added, each `{ kind, nameMessage, added, configurations }` and each configuration `{ id, name, folder?, state?, running, runId?, debug? }` (its latest run; `state` absent before it first ran); `available` lists the kinds not added yet as `[id, nameMessage]` |
| `run.services.add` | `{ kind? \| configurationId? }` | The list, with every configuration of the kind (Add Service → Run Configuration Type) or that one configuration shown. The window keeps the choice |
| `run.services.remove` | `{ kind? \| configurationId? }` | The list without that kind or that configuration added on its own |
| `run.services.hide` | `{ configurationId }` | The list, the configuration hidden (Hide Configuration) |
| `run.services.restore` | `{}` | The list with every hidden configuration shown again (Restore Hidden Configurations) |
| `run.process.rerun` | `{ runId, waitMs? }` | `{ runId }` of the new run, which stops that run if it still runs and takes its tab; with `waitMs`, also its `summary` once it ended, as `run.process.start` |
| `run.output.get` | `{ runId, from?, waitMs? }` | `{ lines, next, state, exitCode?, error? }`; each line has `stream` (`stdout`, `stderr`, `system`), `text` without escape sequences and `links` (`range`, `file`, `line`, `column`) |
| `run.tests.get` | `{ runId, summary?, failedOnly?, test?, package? }` | Without the options `{ tree, counts, done }`: the run, packages, tests and subtests with `status`, `durationMs` and console `lines` (ranges of console line indexes). With `summary`: `{ summary: { state, done, exitCode?, error?, counts, durationMs? } }`, under 300 bytes. With `failedOnly`: `{ summary, failed }`, each failed test (not a parent that failed only through a subtest) `{ package, test, durationMs?, file?, line?, column?, message }`: the first place its output names whose file exists (relative to the project when in it, one-based), and its output from there, at most 6 lines and 600 characters. With `test` (and `package` when two packages have a test of that name): `{ output: { package, test, status, durationMs?, output } }`, that test's console lines |
| `run.tests.rerunFailed` | `{ runId, waitMs? }` | `{ runId }` of a run of the failed tests only; with `waitMs`, also its `summary` |
| `run.tests.rerun` | `{ runId, test?, package? }` | `{ runId }` of a run of that test with the same settings, or of every test of `package` when `test` is absent |
| `run.tests.source` | `{ runId, test, package? }` | `{ path, line }`: where the test is written, line from 1; a subtest gives its test function. Refused for a provider that cannot say, and for a file hidden from agents |
| `run.beforeLaunch.types` | `{}` | `{ types }`: each type of before-launch task with its `id`, `name`, `fields`, `defaults`, and the `provider` that adds it when it is not the IDE's own |
| `run.beforeLaunch.list` | `{ configurationId }` | `{ configurationId, tasks, showPage }`: each task with its `index`, `type`, options and `label` |
| `run.beforeLaunch.add` | `{ configurationId, task, index? }` | The list, with the task (`{ type, ...options }`) at the end or at `index` |
| `run.beforeLaunch.update` | `{ configurationId, index, task }` | The list, the task at `index` replaced |
| `run.beforeLaunch.remove` | `{ configurationId, index }` | The list without the task |
| `run.beforeLaunch.move` | `{ configurationId, index, up }` | The list, the task one place up or down |
| `run.beforeLaunch.showPage` | `{ configurationId, show }` | The list; Show this page on or off |
| `run.beforeLaunch.page` | `{ configurationId? }` | `{ show? }`: the configuration (the given one, else the selected one) whose dialog a start from the window opens first |
| `run.anything.list` | `{ query? }` | `{ items }`: what Run Anything (⌃⌃) suggests, in its order: the window's recent commands, newest first, then the run configurations whose names match (permanent and adopted first, temporary last), then the commands the run providers offer for the typed prefix (`go`, `cargo`, `mvn`, `gradle`); each `{ kind, text, configurationId?, temporary?, adopted?, description? }`, `kind` `recent`, `configuration`, `command` or `help`. `?` lists the prefixes |
| `run.anything.run` | `{ text? \| configurationId?, debug?, context?, path? }` | `{ way, runId?, sessionId?, terminalId?, cwd? }`: runs a configuration by name or id (it becomes the selected one; `debug: true` debugs it, as ⇧⏎), else a provider's command in the Run tool window, else the line in a new Terminal tab (`way` `configuration`, `command` or `terminal`), and remembers it among the recent commands (50, kept with the window). A command runs in the project's folder, the module of the current file or `path` (`context: "module"`) or its folder (`"file"`, as ⌥⏎) |

A run starts its configuration's programs in order, each only when the one before exited with 0,
with the IDE's environment, the toolchain's and the configuration's variables, in the configuration's
folder; the console shows each command line before its output and ends with `Process finished with
exit code N` (or `Process stopped`). A run's `state` is `running`, `finished`, `stopped` or `failed`
(a program could not be started; `error` says why) or `beforeLaunchFailed` (a before-launch task
failed and the configuration did not start; `error` names the task and why). Besides the language modules' kinds, every IDE
runs the kind `npm` (a `package.json` script, a program of the project's packages or an install,
with the project's package manager; [run-configurations.md](run-configurations.md#npm)), whose
targets are the scripts of a `package.json`. To follow a run, call
`run.output.get` again with `from` set to the last `next` and a `waitMs` (at most 10 000) until the
state is no longer `running`. An agent that only needs the outcome starts the run with
`waitMs` and then asks `run.tests.get` with `failedOnly`: measured on this repository's
`zen-workbench` tests (605), the whole tree is 173 KB, the summary 164 bytes and the failed tests with
where and why under 1 KB ([ADR-0090](adr/0090-one-based-places-and-lean-test-results.md)). Each run has a folder of its own for what it builds, removed when it ends, and closing the
window kills the programs that still run.

### Before-launch tasks

A configuration's `common.beforeLaunch` tasks run in order before its programs, in the same run,
each after a `#before-launch <task>` console line ([run-configurations.md](run-configurations.md#before-launch),
[ADR-0202](adr/0202-before-launch-tasks-are-an-extension-point.md)). The edit commands refuse a
configuration adopted from another IDE's files. `run.process.start` with `waitMs` waits through the
tasks too; Show this page never applies to it.

Task types are an extension point. A language module or plug-in adds one through its run provider:

- `RunProvider::task_kinds` returns its kinds as `KindSpec`s (the id, which is the task's `type`;
  the message id of its name; its fields, drawn by the dialog as a configuration's fields are;
  its defaults);
- `RunProvider::task_lines(task, context)` returns the programs a task of its kind runs, or `None`
  for a kind it does not know; the platform runs them in the run's console, and an exit code other
  than 0 fails the task;
- `RunProvider::adopt_task(task, root)` maps a task of an adopted run configuration file, by its
  id, to one of its kinds.

A feature of the platform that has to act on the session (start a run of its own, dispatch its
command) implements `zen_workbench::before_launch::TaskType` instead and registers it with
`before_launch::register_type(session, Arc::new(...))` when the window starts: `spec()` as above,
`start(session, task, launch)` answering `Started::Done`, `Started::Lines(programs)`,
`Started::Run(run_id)` (a run it started, for example with the function behind its `*.run`
command; the launch waits for it and fails unless it ends with exit code 0),
`Started::Wait(poll)` (work it goes on with on the session: the launch calls `poll` at each step
until it answers `Some(Ok(()))`, or `Some(Err(reason))` to fail the task; Run File Watchers,
`fileWatchers`, waits for its runs this way) or `Started::Skipped(reason)`, and optionally `summary()` for the list's label. Maven goals
(`maven.goal`, behind `maven.goal.run`) and Gradle tasks (`gradle.task`, behind `gradle.task.run`)
are provider kinds; Build Artifacts (`buildArtifacts`, behind `build.artifacts.build`) is a
registered type of the artifacts feature, and an Ant target (`ant.target.run`) is added the same way
by the module that owns it.

### File watchers

File watchers ([ADR-0207](adr/0207-file-watchers.md)) run a program when matching files are saved
or changed on disk. The window steps them by itself; the person sets them up, and agents run them
with these commands (never on, or into, a file hidden from agents: code `redacted`):

| Command | Parameters | Result |
|---|---|---|
| `run.fileWatchers.list` | `{}` | `{ watchers, projectFile?, globalFile? }`: the project's watchers (`.zen/fileWatchers.json`) then the global ones, each with `level` and every option |
| `run.fileWatchers.templates` | `{}` | The templates + offers, each `{ origin, watcher }` |
| `run.fileWatchers.save` | `{ watcher, level? }` | `{ id, level }`; an empty or unknown id adds a watcher (`project` by default) |
| `run.fileWatchers.replace` | `{ watchers }` | Both lists replaced, as OK in Settings does; the new lists |
| `run.fileWatchers.remove` / `copy` | `{ id, level? }` | Nothing / the copy's `{ id, level }` |
| `run.fileWatchers.move` | `{ id, level?, up }` | `{ moved }`, within its level |
| `run.fileWatchers.enable` | `{ id, level?, enabled? }` | Nothing; Enable again in the failure notification |
| `run.fileWatchers.import` / `export` | `{ path, level? }` / `{ path, watchers? }` | `{ ids }` added / `{ count }` written |
| `run.fileWatchers.run` | `{ paths?, id?, level?, waitMs? }` | `{ queued, finished, done }`: runs the enabled watchers whose file type and scope match each file (the active one by default) now; with `waitMs` waits for them and answers how each ended (`state`, `exitCode`, `problems`, `disabled`) |
| `run.fileWatchers.status` | `{}` | `{ pending, running, queued, finished }`: files waiting for their pause, the run of each watcher, the files queued behind it, the latest ended runs |

Agents may list, run, move, switch off, remove and export watchers; `save`, `replace`, `copy`,
`import` and `enable` switching one on are refused for them with code `agentNotAllowed`, as a
watcher runs a program on every save.

A run configuration runs them before it starts with the before-launch task `fileWatchers`
(`{ watcher, files }`, [run-configurations.md](run-configurations.md#before-launch)).

Project watchers run only in a trusted project ([ADR-0205](adr/0205-trusted-projects.md)): until
then the window skips them, and `run.fileWatchers.run` that would run only project watchers is
refused with code `notTrusted`.

A watcher's run is an external tool's run (`run.process.list` shows it, `run.output.get` reads
it), and its output filter matches are problems that `problems.file.list` and
`problems.project.list` list with the watcher's name as `source`.

## Profiles

Runs with the profiler and what they recorded ([ADR-0273](adr/0273-profiles-one-model-read-by-the-run-providers.md)):

| Command | Parameters | Result |
|---|---|---|
| `profile.run.start` | `{ configurationId? \| context? \| configuration?, waitMs?, kind? }` | `{ runId, summary?, profile? }`: as `run.process.start`, with `kind` `cpu` (the default) or `memory`; Go test configurations only. With `waitMs` the answer waits for the profile as well: `profile` is `{ runId?, name, metrics, defaultMetric?, samples, frames, durationMs, error? }` |
| `profile.file.open` | `{ path, waitMs? }` | `{ profile? }`: reads a pprof file, gzipped or not, off the UI thread and shows it; for an agent, refused when the file is hidden from agents |
| `profile.report.get` | `{ metric?, limit? }` | `{ shown, loading, revision, summary?, metric?, total, functions, omitted }`: the costliest functions by their own cost, each `{ frame, function, file?, line, flat, flatPercent, cumulative, cumulativePercent }`; 50 unless `limit` says otherwise |
| `profile.tree.get` | `{ metric?, depth?, minPercent? }` | `{ revision, metric, total, root }`: the call tree, which a flame graph also draws; each call is `{ frame?, function?, value, percent, children, omitted }`, `omitted` counting the calls `depth` (12, at most 64) and `minPercent` (1) left out |
| `profile.frame.open` | `{ frame }` | `{ path, line }`: opens a function's source at the line it starts at; refused when the file is not on this computer |

## Code coverage

Runs with coverage and what they measured ([ADR-0208](adr/0208-code-coverage-model-and-providers.md)):

| Command | Parameters | Result |
|---|---|---|
| `coverage.run.start` | `{ configurationId? \| context? \| configuration?, waitMs? }` | `{ runId, summary?, coverage? }`: as `run.process.start` (`confirmRerun`, the project's trust and before-launch tasks, ADR-0205); once the run ended (failed tests too), its data is read and shown. With `waitMs` the answer waits for the data as well: `coverage` is `{ runId, name, counts, percent, files, error? }` |
| `coverage.run.supports` | `{ configurationId? \| configuration? }` | `{ supported }`: whether the configuration (the selected one by default) can run with coverage; a missing tool (`cargo-llvm-cov`) is reported when the run starts |
| `coverage.report.get` | `{ path? }` | `{ shown, loading?, revision, summary?, files }`: each measured file `{ path, counts, percent }`, `counts` being `{ covered, partly, notCovered, branches? }`; `path` narrows it to a folder or a file |
| `coverage.file.get` | `{ path, known? }` | `{ revision, unchanged, lines, counts? }`: the file's measured lines `{ line, hits, status, branches? }` (one-based; `status` `covered`, `partly` or `notCovered`); with `known` equal to the revision, `unchanged` and no lines |
| `coverage.report.hide` | `{}` | `{ hidden }`: the coverage shown is dropped |
| `coverage.report.export` | `{ folder }` | `{ index }`: the HTML report written to the folder; for an agent, refused when the folder, or a measured file whose source the report would copy, is hidden from agents |
| `java.jacoco.download` | `{}` | `{ version, folder, task? }`: downloads the JaCoCo agent and command-line jars from Maven Central (checked against their SHA-512) into the IDE's data folder, in a background task, for Java and Kotlin runs with coverage when the local Maven repository and the Gradle cache have none; the network is used only then |

The window action `view.toolWindow.coverage` shows or hides the Coverage tool window.

`java.bytecode.show { path?, line?, waitMs? }` (View → Show Bytecode, ADR-0230) answers `{ class,
compiled, classFile?, document?, text?, build? }`: the binary name of the Java or Kotlin class at the
place (the active editor's caret by default; Kotlin code outside a class is in the file's class, such
as `CartKt`), and runs the project JDK's `javap -c -p -v` on its compiled
class in the background, opening the output as a read-only document in the right split that then
follows the caret; with `waitMs` (at most a minute) `text` is javap's output. A class of a Maven or
Gradle module that is not compiled yet answers `compiled: false` with `build` (`maven.goal.run` or
`gradle.task.run` and its parameters); a Java scratch file or a Java file outside Maven and Gradle is
compiled alone with `javac` first, while a Kotlin file there is refused (it must be built).

## Text with a preview

A file whose kind has a preview (an SVG, [ADR-0137](adr/0137-text-with-preview.md)) is a text
document like any other (`get_document_text`, `editor.text.*`, problems); the window shows its
preview beside it, as the mode of its kind says. An SVG is no longer an image document:
`editor.image.info` refuses it, and `editor.image.zoom` zooms its preview while it is shown.

| Command | Parameters | Result |
|---|---|---|
| `editor.preview.getMode` | `{ path?, waitMs? }` | `{ path, kind, mode, orientation, scope, state, error?, size? }`: the open file's (the active one without `path`) kind (`svg`, `markdown`), the mode it is shown in (`editorOnly`, `editorAndPreview`, `previewOnly`), where that mode comes from (`scope`: `file`, its own, set by its switch; `type`, set for its kind; `default`, the kind's default), and the preview's state: `ready` (with `size`, the picture's own size), `rendering` (nothing drawn yet, or the text changed and its picture is being drawn), `invalid` (with `error`) or `notOpen`. With `waitMs` (at most 10 000) it waits for the current text to be drawn. A file without a preview is refused |
| `editor.preview.setMode` | `{ mode, path?, scope? }` | the same, after setting the mode, kept in `previews.json` of the data folder: with `scope: "file"` (the default, as the switch) the file's own (the last 500 files are remembered); with `scope: "type"` the mode of every file of its kind that has none of its own, dropping this file's own |
| `editor.preview.editorOnly`, `editor.preview.editorAndPreview`, `editor.preview.previewOnly` | `{}` | the same for the active file: the switch's three buttons, which set the file's own mode |
| `editor.preview.setOrientation` | `{ orientation, path? }` | the same, after setting how the editor and the preview share the file's editor in `editorAndPreview`: `vertical` (side by side, the default) or `horizontal` (one above the other), kept per file in `previews.json` (`orientations`, the last 500 files) |
| `editor.preview.toggleOrientation` | `{}` | the same for the active file: the **Change Split Orientation** button, shown beside the switch in `editorAndPreview` |

### Markdown

A Markdown file's preview is a kind of its own (`markdown`, [ADR-0138](adr/0138-markdown-preview.md));
`editor.preview.getMode` gives its `blocks` count once drawn. `markdown` in `get_settings` and
`set_settings` holds Languages & Frameworks → Markdown: `previewLayout` (the same value as
`editor.preview.setMode` with `scope: "type"`, kept in `previews.json`; changing either changes the
other), `syncScroll` and `previewFontSize` (8 to 32); `run_action` with `view.settings.open` and
`params: { "page": "markdown" }` opens the page.

| Command | Parameters | Result |
|---|---|---|
| `markdown.preview.render` | `{ path?, format? }` | `{ path, headings, text? \| html? \| rendered? }`: what the preview shows of a Markdown file (the active one by default; an open file as the editor has it), as plain text (`format` `text`, the default: list markers, `[x]` boxes, `>` quotes and `[WARNING]` alerts kept), safe HTML (`html`, `data-line` on every block) or the blocks as JSON with their source lines and nesting (`blocks`); `headings` with `level`, `text`, `anchor` and `line` |
| `markdown.task.toggle` | `{ line, path? }` | `{ checked, text }`: ticks or unticks the task list item on a line (from 1) of an open file, as a click on its checkbox does; one undo step |
| `markdown.images.allow` | `{ host, allowed? }` | `{ hosts }`: lets the open project's previews load remote images from a host (or an image's address), `allowed: false` to stop; kept per project. Nothing remote is fetched before |
| `markdown.images.list` | `{}` | `{ hosts }` the open project allows |
| `markdown.preview.getSelection` | `{ path? }` | `{ path?, text, markdown }`: what is selected in the preview, as ⌘C copies it (`text`: list markers, `[x]` boxes, table cells split by tabs) and as the Markdown source lines of the selected blocks; empty when nothing is selected or the preview is not drawn |

| `markdown.mermaid.list` | `{ path?, svg? }` | `{ path, diagrams }`: the file's ```` ```mermaid ```` blocks in reading order, each `{ line, drawn, error?, svg? }`: its fence's line, whether the preview draws it, why not (the preview then shows its source with that note), and with `svg` the diagram as SVG. Nothing is fetched |
| `markdown.format.bold`, `.italic`, `.strikethrough`, `.code` | `{}` | `{ changed, selection }`: wraps every selection (the word at a caret) of the active Markdown file in the style's markers, or takes them off when it has the style, as ⌘B, ⌘I, ⇧⌘S and ⇧⌘C do; one undo step. `selection` is the primary selection's `[start, end]` in characters. Not available outside Markdown |
| `markdown.format.link` | `{ url? }` | the same: makes the selection the text of a link (⇧⌘U); without `url` the caret goes where the address is typed |
| `markdown.format.heading` | `{ level }` | the same: the selected lines become headings of level 1 to 6, or plain text with 0 |
| `markdown.format.list` | `{ kind }` | the same: the selected lines become items of a `bullet`, `numbered` or `check` list, or plain text when they all already are of that kind |

`markdownHideFloatingToolbar` in `get_settings` and `set_settings` is Advanced Settings → Markdown →
Hide the formatting toolbar over a selection. The five shortcuts are keymap actions of the same ids, in the keymap context
`markdown`: only while a Markdown editor has the keyboard.

The window actions `view.markdownPreview.selectAll`, `view.markdownPreview.copy` and
`view.markdownPreview.copyMarkdown` (through `run_action` or `dispatch_command`) select all of the
active file's preview, or copy its selection as plain text or as Markdown; each answers
`{ path, text }`, and fails when the active file's preview has not been drawn.

## HTTP Client

The requests of `.http` and `.rest` files are commands of the project window
([ADR-0061](adr/0061-http-client.md); the guide's [HTTP Client](guide/20-http-client.md) page says
what the user sees). Lines count from 1, as in every answer an agent reads (ADR-0090).

| Command | Parameters | Result |
|---|---|---|
| `http.requests.list` | `{ path }` | `{ requests, variables, environments, env, environmentFolder?, environmentError? }`: each request with `index`, `name?`, `method`, `url` and `headers` as written, `body?`, `line` (the request line, where the gutter's run icon is), `startLine`, `endLine` and `noRedirect`; the file's `@name = value` variables; the environments of the nearest `http-client.env.json` / `http-client.private.env.json` and the selected one. Never sends anything |
| `http.environment.select` | `{ env? }` | `{ env }`; the environment requests of the window run with; none when absent or empty |
| `http.request.run` | `{ path, index? \| name?, env?, waitMs? }` | The exchange: `id`, `file`, `index`, `name?`, `environment?`, `method`, the resolved `url`, `requestHeaders` and `requestBody?`, `started` (ms since 1970), `state` (`running`, `done`, `failed`), `durationMs?`, `response?` (`status`, `reason`, `version`, `headers`, `contentType?`, `body` (text, at most 1 MiB), `size`, `truncated`, `binary`) and `error?`. `index` or `name` picks the request (either may be left out when the file has one); `env` absent is the selected environment, `""` none. With `waitMs` (at most 60 000) it waits for the answer. Unresolved variables fail with the code `unresolvedVariables` and nothing is sent; so do a scheme other than `http`/`https` and a `< file` body outside the project. Values from `http-client.private.env.json` stay `{{name}}` in `url`, `requestHeaders` and `requestBody`: they are sent, never returned or kept |
| `http.responses.list` | `{ path?, limit? }` | `{ responses, historyFile? }`: the exchanges, newest first, running ones too, and the history file in the IDE's data folder |
| `http.response.get` | `{ id, waitMs? }` | One exchange; with `waitMs` it first waits while the request runs |

**`http.request.run` is the only command that sends anything**, and only the request asked for: the
gutter's run icon runs it for a person, an agent runs it itself. An agent can do so only once the
user has turned the MCP server on (the consent that also lets agents start programs and type into
terminals); there is no other prompt. Find Action does not list it, as it needs a file. The history is kept in
`<data>/http-client/<project folder>-<hash>/history.json` (the newest 50, at most 16 MiB of bodies),
never in the project or in `.idea`. The Services tool window (`view.toolWindow.services`, ⌘8) shows it; `get_window_state`
names it `services`.

## Debugging

Debug sessions, breakpoints and watches are commands of the project window too
([ADR-0052](adr/0052-debugger-zen-dap-and-delve-over-reverse-dap.md)). Lines and columns count from
0 in every parameter and result; `sessionId` defaults to the current session (the one started last
unless the Debug tool window chose another) and `threadId` to its stopped thread.

| Command | Parameters | Result |
|---|---|---|
| `debug.session.start` | `{ configurationId? \| context? \| configuration? }` | `{ sessionId, runId, runs?, errors? }`; the parameters mean what they mean for `run.process.start`. A compound debugs each of its configurations that its provider can debug and runs the others; `runs` lists every run it started and `sessionId` is the first session's. Modified documents are saved first. Fails with the error `code` `debuggerMissing` when the debugger (Delve) is not installed |
| `debug.session.stop` | `{ sessionId? }` | Nothing; sends `disconnect` with the program ended and kills the debugger if it has not exited 2 s later; during the build it stops the build |
| `debug.session.list` | `{}` | `{ sessions }`: each session's `sessionId`, `runId`, `name` and `state` |
| `debug.session.get` | `{ sessionId?, waitMs? }` | `{ state, reason?, threadId?, frame?, threads, stops }`: `state` is `starting`, `running`, `stopped` or `ended`; `reason` is why it stopped (`breakpoint`, `step`, `pause`, `exception`, `entry`) or ended (`exited`, `stopped`, `failed`, `buildFailed`, `beforeLaunchFailed`); `frame` is `{ path, line, name }` of the selected frame; `stops` counts the stops; `values` is raised at each stop and after `debug.variable.set`; `support` is `{ setValue, smartStepInto, resetFrame }`, what the connected debugger can do; `reason` is `restart` after `debug.frame.reset`; with `waitMs` (at most 10 000) it first waits while the session starts, runs or is being stopped |
| `debug.execution.resume` | `{ sessionId?, threadId? }` | Nothing; the program runs to the next breakpoint or its end |
| `debug.execution.pause` | `{ sessionId?, threadId? }` | Nothing; the session stops with the reason `pause` |
| `debug.execution.stepOver` | `{ sessionId?, threadId? }` | Nothing; stops at the next line with the reason `step` |
| `debug.execution.stepInto` | `{ sessionId?, threadId? }` | Nothing |
| `debug.execution.stepOut` | `{ sessionId?, threadId? }` | Nothing |
| `debug.execution.runToCursor` | `{ path?, line? }` | Nothing; the program runs to the line (the caret's by default) or an earlier breakpoint |
| `debug.execution.forceRunToCursor` | `{ path?, line? }` | Nothing; the program runs to the line (⌥⌘F9) with every line, function and data breakpoint withheld from the debugger until it stops |
| `debug.execution.forceStepOver` | `{ sessionId?, threadId? }` | Nothing; Step Over (⌥⇧F8) with the breakpoints withheld until the program stops |
| `debug.execution.forceStepInto` | `{ sessionId?, threadId? }` | Nothing; Step Into (⌥⇧F7): DAP debuggers step into every function, so it does what `debug.execution.stepInto` does |
| `debug.execution.stepInTargets` | `{ sessionId?, frameId?, waitMs? }` | `{ targets, pending? }`: the calls on the line of the frame (the selected one by default) that Smart Step Into can go into, `[{ id, label, line?, column?, endLine?, endColumn? }]` with lines and columns from 0; DAP `stepInTargets`, unavailable unless `debug.session.get`'s `support.smartStepInto`. `waitMs` works as for `debug.variables.list` |
| `debug.execution.smartStepInto` | `{ targetId, sessionId?, threadId? }` | Nothing; Smart Step Into: DAP `stepIn` with the `targetId` of a target from `debug.execution.stepInTargets`; unavailable unless `support.smartStepInto` |
| `debug.frames.list` | `{ sessionId?, threadId?, start?, count? }` | `{ frames }`: `[{ id, name, path?, line, column, hint?, async?, canRestart? }]`, the innermost first, 20 by default. `hint` is `label` for a separator the debugger puts before the frames of an awaited or scheduled call (its async stack trace) and `subtle` for a frame it plays down; `async` marks the frames below such a separator; `canRestart` is true for a frame `debug.frame.reset` can restart |
| `debug.frame.select` | `{ frameId }` | Nothing; variables, evaluation and watches use that frame |
| `debug.frame.reset` | `{ frameId?, sessionId? }` | Nothing; Reset Frame (formerly Drop Frame): DAP `restartFrame` on the frame (the selected one by default), the session then stops with the reason `restart`; unavailable unless `support.resetFrame`, refused for a frame without `canRestart` |
| `debug.variables.list` | `{ sessionId?, frameId?, waitMs? }` or `{ reference, start?, count?, raw?, waitMs? }` | `{ variables, pending? }`: a frame's scopes, or the children of a scope or variable: `[{ name, value, type?, reference, indexed?, named?, evaluateName?, entry? }]`; a `reference` of 0 has no children. A collection, map or array lists its elements (a debugger that can show them, such as java-debug) and a map entry carries `entry: { key, value }`; `raw: true` lists its fields instead (`debug.variables.raw`'s choice when absent). A value the debugger marks lazy is fetched first. With `waitMs` (at most 10 000; 0 does not wait) it answers `pending: true` while the debugger works: call again with the same parameters |
| `debug.variables.raw` | `{ raw }` | `{ raw, supported }`: Show Raw Fields of Every Collection: collections, maps and arrays in Variables, watches and Evaluate show their fields (`raw: true`) or their elements (the default); the setting `collectionViews` (Settings → Debugger → Data Views → Enable alternative view for Collections classes) holds the opposite. `supported` says whether the current session's debugger can switch (ADR-0231) |
| `debug.stream.trace` | `{ sessionId?, frameId?, path?, line?, limit?, waitMs? }` | `{ chain, columns, pending? }`: Trace Current Stream Chain for the Java stream chain of the statement on the stopped line (or `path`, `line` from 0): `columns` are `[{ label, kind, expression?, values, count?, result?, error?, pending? }]`, the stream's source and each operation (`kind` `elements`: the values after it, `prefix.toArray()` evaluated in the frame, at most `limit`, 100 by default), then the terminal operation (`result`, evaluated as written; `notRun` for `forEach` and the like). Refused outside Java, for a stream kept in a variable, with `peek`, and for an infinite stream without `limit`/`takeWhile` (whose columns before it are `unbounded`). `waitMs` works as for `debug.variables.list`, with the columns known so far |
| `debug.coroutines.list` | `{ sessionId?, waitMs? }` | `{ coroutines, more?, frame?, unavailable?, message?, detail?, pending? }`: the Kotlin coroutines of a stopped Java or Kotlin program as kotlinx-coroutines-debug sees them, `[{ state, name?, dispatcher?, job, frames }]` (at most 200, 32 frames each; `more` when there are more). Read-only: one fixed dump of the coroutine debug agent (never other text), evaluated in the first of the top 8 frames of the thread a breakpoint or a step stopped that can run it (`frame` names it). `unavailable` says why there is no list: `noEvent` (stopped by Pause), `notJvm`, `notInstalled` (no agent: Track coroutines), `noLibrary` or `noFrame`. `waitMs` works as for `debug.variables.list` |
| `debug.expression.evaluate` | `{ expression, frameId?, context?, waitMs? }` | `{ result, type?, reference, indexed?, named?, pending? }`; `context` is `repl` by default and such expressions become the project's recent expressions; `hover` is refused when the debugger does not evaluate for hovers. `waitMs` works as for `debug.variables.list`; without it the call waits up to 10 s, then answers `pending` |
| `debug.variable.set` | `{ reference?, name, value, evaluateName?, frameId?, sessionId?, waitMs? }` | `{ value, type?, reference, pending? }`: Set Value (F2 in Variables): DAP `setVariable` on the variable `name` of the scope or parent `reference` when the debugger has it, otherwise `setExpression` on `evaluateName` (or on `name` without a `reference`) in the frame; unavailable unless `debug.session.get`'s `support.setValue`. The variables are read again after it; `waitMs` works as for `debug.variables.list` |
| `debug.inlineValues.get` | `{ path?, sessionId?, waitMs? }` | `{ path?, lines, pending? }`: the values the editor shows at line ends while the program is stopped: `lines` are `[{ line, values: [{ name, value, changed }] }]`, the lines of the selected frame's function up to the stopped line that name a variable of the frame's scopes (not after a `.`, in a string or a `//` comment), `changed` when the value differs from the previous stop in the same function; empty for another file than the frame's. `waitMs` works as for `debug.variables.list` |
| `debug.expression.history` | `{}` | `{ expressions }`: the project's recent expressions, the newest first, at most the setting `recentExpressions` (50); kept with the window's state |
| `debug.executionPoint.show` | `{ sessionId? }` | `{ path, line, frameId }`: selects the top frame of the thread the program stopped in and opens its line (Show Execution Point, ⌥F10) |
| `debug.breakpoint.toggle` | `{ path?, line? }` | `{ breakpoint }`, `null` when one was removed; at the caret of the active editor by default |
| `debug.breakpoint.set` | `{ path, line, enabled?, condition?, suspend?, logHit?, logStack?, logExpression?, removeOnceHit?, hitCondition?, dependsOn?, afterHit?, group?, description? }` | `{ breakpoint }`; what is absent stays, an empty text clears it, `dependsOn: null` makes it independent. `suspend` is `all`, `thread` or `none` (log only), `afterHit` is `disableAgain` or `leaveEnabled` |
| `debug.breakpoint.toggleTemporary` | `{ path?, line? }` | `{ breakpoint }`: a breakpoint with `removeOnceHit` (⌥⇧⌘F8), or `null` when one was removed |
| `debug.breakpoint.toggleEnabled` | `{ path?, line? }` | `{ breakpoint }`: enabled or disabled (⌥-click); a line without one is refused |
| `debug.breakpoint.move`, `debug.breakpoint.copy` | `{ path, line, toLine }` | `{ breakpoint }` at `toLine` with every property (a drag, a ⌘-drag in the gutter); moving onto a breakpoint is refused, copying replaces it |
| `debug.breakpoint.removeOthers`, `debug.breakpoint.disableOthers` | `{ path?, line? }` | `{ count }`: every other line breakpoint of the project removed or disabled |
| `debug.breakpoints.mute` | `{ muted? }` | `{ muted }`; the other way round without `muted`. While muted every session gets empty breakpoint lists; not saved |
| `debug.functionBreakpoint.add`, `debug.functionBreakpoint.remove` | `{ name, enabled?, condition?, hitCondition? }`, `{ name }` | `{ breakpoint }`, nothing: a function breakpoint by name, sent to debuggers with `supportsFunctionBreakpoints` |
| `debug.exceptionBreakpoint.add`, `debug.exceptionBreakpoint.remove` | `{ filter, enabled?, condition? }`, `{ filter }` | `{ breakpoint }`, nothing: an exception filter of `offered`, its condition sent when the debugger takes one |
| `debug.dataBreakpoint.add` | `{ reference?, name, frameId?, accessType?, condition?, hitCondition? }` | `{ breakpoint }`: watches a variable (its scope's or parent's `reference`) or an expression of the stopped program; only debuggers with `supportsDataBreakpoints`, for the session's length |
| `debug.expression.complete` | `{ path, line, text, caret? }` | `{ pending, start, items, error? }`: completes an expression field (a breakpoint's condition, an expression to log) as if `text` were typed at `line`: the language server gets a hidden copy of the file with the text inserted there, the file and its document are not changed; `items` are `[{ label, insert, kind, detail? }]` replacing the field's text from `start` to the caret; call again while `pending` |
| `debug.dataBreakpoint.set`, `debug.dataBreakpoint.remove` | `{ dataId, enabled? }` | Enables, disables or removes a data breakpoint |
| `debug.breakpoint.remove` | `{ path, line }` | Nothing |
| `debug.breakpoint.list` | `{ path? }` | `{ breakpoints, functions, exceptions, offered, data, muted, support? }`: the line breakpoints `[{ path, line, enabled, verified?, message?, condition?, …, waiting? }]` with the properties of `debug.breakpoint.set` (`waiting` while one waits for its `dependsOn`), the function, exception and data breakpoints, the exception filters the debuggers offered, and `support`, what the current debugger takes: `conditions`, `hitConditions`, `logPoints`, `threadSuspend`, `functions`, `data`, `exceptionConditions` |
| `debug.watch.add` | `{ expression }` | Nothing |
| `debug.watch.remove` | `{ expression }` | Nothing |
| `debug.watch.list` | `{}` | `{ watches }`: `[{ expression, value?, type?, reference?, error? }]`, evaluated in the selected frame while the session is stopped |
| `debug.exceptionBreakpoint.set` | `{ filters }` | `{ enabled, offered }`: turns on exactly these exception filters (Java: `caught`, `uncaught`) for the current session and the next ones |
| `debug.exceptionBreakpoint.list` | `{}` | `{ enabled, offered }`: the filters on, and `[{ filter, label, enabled }]` the current session's debugger offers |

A debug session is a run: its `runId` works with `run.output.get` (the build's command lines and
output, the debugger's command line, then the program's output and `Process finished with exit
code N`), `run.process.stop` stops it, and `run.process.list` marks it with `debug: true`. A
session starts by building with the configuration's own command lines; a failed build ends it
(`buildFailed`) with the errors as links in the console. Then the IDE listens on `127.0.0.1` with a
port the system chose, starts the debugger with that address, and sets the breakpoints before the
program runs. Breakpoints move with the edits of open files (a deleted line takes its breakpoint
along), a running session gets every change at once, and `verified` and `message` come from the
debugger while a session runs. To follow a session, call `debug.session.get` with a `waitMs` after
each start, resume or step until its state is `stopped` or `ended`.

Breakpoint properties the debugger lacks are done by the IDE (ADR-0203): a breakpoint with
`suspend: "none"` is sent as a log point when the debugger has them and only the hit message or an
expression is logged; otherwise the IDE stops, writes the log lines to the run's console and
resumes. A breakpoint with `dependsOn` is sent only after its master's hit in a session. The
window's own actions are `view.breakpoints.open` (View Breakpoints…, ⇧⌘F8; `{ path?, line? }`
selects one), `view.breakpoint.properties` (the gutter popup, `{ path?, line?, focus? }` with
`focus` `condition` or `logExpression`), `view.breakpoint.addConditional` and
`view.breakpoint.addLogging` (a breakpoint at the caret and its popup); `get_window_state` names
them `breakpoints` and `breakpointProperties`. The setting `removeBreakpointByDrag` is Settings →
Debugger → Remove breakpoint.

Evaluation in the window: `view.evaluateExpression.open` (Evaluate Expression…, ⌥F8; `{ expression? }`,
the selection or the name at the caret by default) opens a non-modal Evaluate dialog, and
`view.quickEvaluate.open` (Quick Evaluate Expression, ⌥⌘F8, ⌥-click; `{ expression? }`) a value
popup at the expression; both are refused unless a session is stopped. `get_window_state` names them
`evaluateExpression` and `quickEvaluate`, and a value tooltip `valueTooltip`. The dialog, the popup,
the tooltip, the field of the Variables pane and the variables tree ask `debug.expression.evaluate` and
`debug.variables.list` with `waitMs: 0`, so a frame never waits for the debugger; the tooltip uses the
`hover` context. `view.smartStepInto.open` (Smart Step Into, ⇧F7) asks `debug.execution.stepInTargets`
with `waitMs: 0` and shows the calls in a popup under the caret, `smartStepInto` in `get_window_state`,
the chosen call framed in the editor; it steps at once with one call (`{ stepped: true }`) and is
refused unless a session is stopped with `support.smartStepInto`. `view.streamTrace.open` (Trace
Current Stream Chain, Run → Debugging Actions; `{ path?, line? }`) opens the Stream Trace dialog,
`streamTrace` in `get_window_state`, filled by `debug.stream.trace` with `waitMs: 0`; it is refused
when no session is stopped or when the line has no stream chain to trace, and closes when the
program goes on. A collection row's menu in Variables and in the Evaluate tree has Show Raw Fields
(that row) and, in Variables, Show Raw Fields of Every Collection (`debug.variables.raw`). The settings `valueTooltip` and `valueTooltipDelay` are Settings → Debugger → Data
Views, `recentExpressions` is Advanced Settings → Debugger. `inlineValues` (Settings → Debugger → Data
Views → Show values inline) and `runToCursorInlay` (Advanced Settings → Debugger → Show a Run to
Cursor button on lines while debugging, the button at the start of a hovered line that runs `debug.execution.runToCursor`) are
on by default; the editor asks `debug.inlineValues.get` with `waitMs: 0` once per stop.

## Background tasks

Long work runs as background tasks of the window, and the status bar and its panel show them.

- `tasks.background.list {}` returns `{ panelShown, selected?, tasks }`, the oldest task first. A task
  is `{ id, title, source, state, fraction?, detail, message?, logLines, started, ended?, cancellable?,
  cancelling?, pausable?, paused? }`, `state` being `running`, `succeeded`, `failed` or `cancelled`.
  `source` is who started it: `vcs`, `inspections`, `maven`, `gradle`, `index`, `language-server`...
- `tasks.log.read { id?, tail? }` returns `{ task, lines }`: what its programs printed (the latest
  task by default).
- `tasks.background.cancel { id? }` stops a task with `cancellable` (the latest by default) and
  answers its id: its programs are interrupted with their process groups and killed after 2
  seconds, a language server gets `window/workDoneProgress/cancel`. The task stays `running` with
  `cancelling` until it has stopped, then ends `cancelled`; a cancelled dependency update has put
  `go.mod` and `go.sum` back.
- `tasks.background.pause { id? }` and `tasks.background.resume { id? }` pause a task with `pausable`
  and resume it (the latest by default) and answer its id. Only indexing can be paused, and it
  cannot be cancelled; `search.index.status` says `paused` as well.
- `tasks.panel.show { id? }`, `tasks.panel.hide {}` and `tasks.panel.toggle {}` show and hide the panel;
  `toggle`, which Window → Processes runs, answers whether it is shown now.
- `tasks.finished.clear {}` forgets the tasks that are over, with their logs.

A language server's work (`$/progress`) is a task of source `language-server` while it lasts, with the
server's percentage as `fraction` and its message as `detail`; it leaves the list when it ends, unless
it was cancelled. Exiting the IDE does not ask about indexing or a server's work.

Quick-fixes that run programs are such tasks too:

- `problems.fixes.list { path?, offset? }` lists the problems with quick-fixes at a place (the caret of
  the active file by default), the worst first, each with its fixes.
- `problems.fix.apply { path?, ids }` starts the fixes and answers `{ job, task }`, the task showing them.
- `problems.fix.status {}` returns how each job is doing, the latest last: `running`, `done` with the
  files it changed, `failed` with what its program printed, or `cancelled`.
- `problems.inspections.refresh {}` makes the inspectors forget what they looked up and look again
  when the files are next inspected.

## Notifications

A project window keeps its notifications (`zen_workbench::notifications`, the Notifications tool
window): updates, plug-in offers, language servers, the debugger, Git, failed commands and the
IDE's own messages. Each has an `id` (larger for newer ones), a `time` in milliseconds since 1970,
a `group` (`general`, `updates`, `plugins`, `languageServers`, `debugger`, `git`, `errors`), a
`type` (`info`, `warning`, `error`), a `title` (empty for a one-line notification), a `body`,
`actions` (`{ id, label, command, params }`: a command or a `view.*` window action), `read` and
`balloon` (false for what an editor banner already shows). A window keeps the last 500; a title
keeps at most 200 characters and a body 4000, a longer one is cut with an ellipsis.

| Command | Parameters | Result |
|---|---|---|
| `ui.notifications.list` | `{ unreadOnly? }` | `{ notifications, unread }`, the newest first; listing does not mark them read |
| `ui.notifications.markRead` | `{ ids? }` | `{ count, unread }`: every notification when `ids` is missing |
| `ui.notifications.clear` | `{ ids? }` | `{ count, unread }`: removes them, every one when `ids` is missing |
| `ui.notifications.action` | `{ id, action }` | The action, with `result`: what its command answered. The notification is marked read; the window runs the command as a click on the link does (`dispatch_command` routes it through the window, like a `view.*` action) |
| `ui.notifications.post` | `{ group?, type?, title?, body?, balloon? }` | `{ id }`; a title or a body is required; any other field, `actions` among them, is refused |

A posted notification never has actions. A link in a notification runs its command when the user
clicks it, so an action an agent or a plug-in could post would let it put any command, a terminal
or a Git push for one, behind a harmless-looking label, and the user's click would run it in the
user's name. Only the IDE's own notifications have actions; an agent that wants the user to run a
command says so in the text, and an agent that wants to run one dispatches it itself.

`view.toolWindow.notifications` shows or hides the tool window (`get_window_state` then has
`"right": "notifications"`); it marks what it shows read. Notifications an agent's failed action
causes are in the list at once. The actions of the IDE's own notifications run window actions:
`view.update.install`, `view.update.later { version }`, `view.plugin.install { id }`,
`view.plugin.ignore { extension }`, `view.plugin.ignoreForProjects { id }`, `view.debugger.install`
and the commands of a project's plug-ins (`plugins.project.setup { plugin }`). `set_settings` turns balloons off
with `notificationBalloons` (all) and `balloonGroupsOff` (a list of group ids).
## TODO

The TODO tool window ([guide](guide/22-tool-windows.md#todo)) lists the TODO and FIXME comments:
the lines of comments, by each file's language (`zen_workbench::todo`, the file's highlighter),
matching `\btodo\b.*` or `\bfixme\b.*` without regard to case, one item a line, its text reaching
to the end of the comment on that line. The files are those of the project's file index, so
excluded folders are left out; open files are read with their unsaved changes. A tab is `project`,
`currentFile`, `scopeBased` (with `scope`, an id of `search.scope.list`) or `currentChangelist`
(the active changelist's files, only in Git).

| Command | Parameters | Result |
|---|---|---|
| `todo.items.list` | `{ tab?, scope?, path?, limit?, filter? }` | `{ tab, scope, files, total, done, scan, error }` at once: `files` by path, each `{ path, relative, items }` and each item `{ line, column, offset, length, text, lineText }` (line and column from 0, `offset` in characters for `navigate.place.goTo`); `error` says why a tab can have no items (no file in the editor, no Git); an unknown scope is an error. `path` is the file of `currentFile`, the file in front by default; at most `limit` items (10000) |
| `todo.tabs.list` | none | `{ tabs, patterns }`: the tabs for the open project and the patterns |
| `todo.scan.start` | as `todo.items.list` | `{ scan }`: lists the items in the background, as the window does; a scan running is cancelled |
| `todo.scan.results` | none | The last scan's list, with its `scan` number and `done` |
| `todo.document.ranges` | `{ path? }` | `[{ start, end, color? }]`: the TODO text of an open file as the editor highlights it, in character offsets, with the pattern's own colour when it has one |
| `todo.commit.check` | `{ paths, filter? }` | `[{ path, line, text }]`: the TODO items in the lines a commit of these files changes against HEAD; a new file counts whole |

The window reads the items again in the background 0.3 s after the last change while it is shown:
an edit, a file changed on disk, another file in front (Current File), another status of Git
(Current Changelist), another tab or scope. The scans of a session remember each file's items with
the file's size and modification time (its text when it is open) and its highlighter, so a scan
reads and highlights again only the files changed since; `todo.items.list` shares that memory. Its
actions, for `run_action`:

| Action | Parameters | Does |
|---|---|---|
| `view.toolWindow.todo` | none | Shows or hides the TODO tool window (no shortcut) |
| `view.toolWindow.coverage` | none | Shows or hides the Coverage tool window (right; it opens by itself when a run with coverage ends) |
| `view.todo.tab` | `tab?`, `scope?` | Shows the window on a tab, with the scope of Scope Based; Current Changelist is refused outside Git |
| `view.occurrence.next` (⌥⌘↓), `view.occurrence.previous` (⌥⌘↑) | none | Next or Previous Occurrence: selects the item after or before the selected one, unfolding its file, and goes to it in the editor; refused while the TODO tool window is hidden |
| `view.todo.filter` | `name?` | Shows the window with the filter `name`, or every item without it; an unknown filter is refused |
| `view.todo.expandAll`, `view.todo.collapseAll` | none | Expand All (⌘+) and Collapse All (⌘−) in the tree |
| `view.todo.previewSource.toggle` | none | Preview Source: the selected item's file beside the tree; answers `{ previewSource }` |
| `view.todo.navigateWithSingleClick.toggle` | none | Navigate with Single Click; answers `{ navigateWithSingleClick }` |

`get_window_state` has `todo: { tab, tabs, scope, items, files, scanning, selected, collapsed,
previewSource, navigateWithSingleClick }`, where `selected` is `{ path, line, text }` for an item or
`{ path }` for a file, and `collapsed` the folded files. The tab, the scope and the two toggles are
remembered with the project (`workspace.json`, `todo`).

## Inspect Code

| Command | Parameters | What it does |
|---|---|---|
| `inspect.code.run` | `scope?` (`project`, `directory`, `files`, `uncommitted`, `custom`), `directory?`, `files?`, `custom?`, `includeTests?`, `inspection?`, `fileMask?`, `profile?` | Inspects a scope in a background task; `tasks.background.cancel` stops it |
| `inspect.cleanup.run` | as `inspect.code.run` | Code Cleanup: applies the fixes of the cleanup inspections the profile leaves on, opens no results; `inspect.code.status` then says what was done in `cleanup` |
| `inspect.cleanup.undo` | `path?` | Undoes the last Code Cleanup's change to a file, or to all its files |
| `inspect.code.status` | `waitMs?` | Moves the run and the fixes along, says how they are doing |
| `inspect.inspections.list` | none | What `inspection` can name: declared inspections, and those only seen in results |
| `inspect.suppress.list` | `path?`, `offset?` | How the problems at a place can be suppressed: `statement`, `function` or `file`, none for an error |
| `inspect.suppress.apply` | `path`, `offset`, `inspection`, `scope` | Writes the language's own mechanism into the file (`//nolint:<id>` in Go, `#[allow(<id>)]` in Rust); one Undo reverts it |
| `inspect.results.list` | `run?` | The items of a run, and the files no server checked, with why |
| `inspect.results.tree` | `run?`, `groupByDirectory?`, `showExcluded?` | The tree the window shows: severity, inspection, file |
| `inspect.results.fixes` | `run`, `item`, `waitMs?` | The quick-fixes of an item; ask again while `pending` |
| `inspect.results.fix` | `run`, `items`, `fix?` | Applies the named fix, or each item's main one |
| `inspect.results.suppress` | `run`, `item`, `scope` | Suppresses an item as `inspect.suppress.apply` does; it and the items of that inspection the suppression covers leave the results |
| `inspect.results.exclude` | `run`, `items`, `excluded?` | Leaves items out of the tree and of exports, or brings them back |
| `inspect.results.rerun` | `run` | Runs it again |
| `inspect.results.export` | `run?`, `format?` (`html`, `xml`), `path` | Writes the results to a file; an agent's only inside the project and never over a hidden file. Hidden files are left out of what an agent lists, exports or fixes |
| `inspect.profile.list` | none | The profiles of the project and the IDE, and the active one |
| `inspect.profile.get` | `profile?` | A profile's inspections: description, cleanup, enabled, severity, modified; `unknown` holds what the IDE does not declare |
| `inspect.profile.select` | `profile` | The profile that applies to this project |
| `inspect.profile.set` | `profile?`, `inspections` (id to `{ enabled?, severity? }`) | Changes inspections in a profile; an empty override brings one back to its defaults |
| `inspect.profile.restore` | `profile?` | Restore Defaults |
| `inspect.profile.copy` | `profile?`, `to` | Copy to IDE, Copy to Project, Duplicate |
| `inspect.profile.rename` | `profile?`, `name` | Renames a profile, never Project Default |
| `inspect.profile.delete` | `profile?` | Deletes a profile, never Project Default |
| `inspect.profile.export` | `profile?`, `path` | Writes a profile to a file |
| `inspect.profile.import` | `path`, `level?` | Adds a profile from such a file |
| `inspect.results.close` | `run` | Forgets a run and closes its tab |

The window adds `view.inspectCode.open`, `view.inspectByName.open`, `view.inspectResults.tab` (`run?`,
no run going back to File and Project Errors), `view.inspectResults.groupByDirectory.toggle` and
`view.inspectResults.showExcluded.toggle`.

## Local History

Local History ([ADR-0060](adr/0060-local-history-store.md), the guide's
[Local History](guide/18-local-history.md)) keeps versions of the project's files in the IDE's data
folder. Paths are absolute paths in the open project, or of a console of the Database workspace
(the folder of consoles, `<data folder>/consoles`, is recorded as a root of its own); a version's
`id` comes from `localHistory.versions.list`.

| Command | Parameters | Result |
|---|---|---|
| `localHistory.versions.list` | `{ path, limit? }` | `{ path, folder, versions }`: the versions of a file, or of every file in a folder (the project's root lists all), and the labels that apply, the newest first (1000 by default). Each has `id`, `time` (milliseconds since 1970), `path?`, `relative?`, `kind` (`save`, `external`, `before`, `action`, `deleted`, `label`), `label?` (the action: `replaceInFiles`, `rename`, `rollback`, `revert`, `lineSeparators`; or the label's text), `title` (as the list shows it), `size`, `binary` and `hasContent` |
| `localHistory.version.get` | `{ path, id, maxBytes? }` | `{ version, contentId, content?, binary, size, truncated }`: the file's text at that version (for a label or a deletion, its last version up to then), at most `maxBytes` (1 MiB by default); no `content` for a binary file |
| `localHistory.version.diff` | `{ path, id }` | `{ diff?, files }`: for a file, the diff viewer's `DiffView` of the version (before) against the current text with unsaved changes (after); for a folder, `files` that changed since: `{ path, relative, status }` with `status` `modified`, `deleted` or `added` (created since) |
| `localHistory.version.revert` | `{ path, id }` | `{ files, kept }`: the files given their content of then (open files change as one undoable edit); for a folder, `kept` lists the files created since, left as they are. Refused for `.idea`, `.run` and `*.iml` files |
| `localHistory.label.put` | `{ label, path? }` | `{ id }`: a label at this moment, on the whole project or on a file or folder |
| `localHistory.changes.record` | `{ paths }` | `{ queued }`: records the content (or the deletion) of files changed outside the IDE; the window calls it for what the file watcher reports |

Recording happens on a worker thread: saves never wait for it, and every query sees what was
recorded before it. The window's `view.localHistory.show { path? }` opens Show History (the active
file, the Project tool window's selection or the project by default), `view.localHistory.putLabel`
opens Put Label…; `get_window_state` lists them as `localHistory` and `localHistoryLabel`.

## Line separators

The status bar's Line Separator widget and File → File Properties → Line Separators
([ADR-0097](adr/0097-line-separators-of-documents.md), the guide's
[Line separators](guide/02-editor.md#line-separators)).

| Command | Parameters | Result |
|---|---|---|
| `file.lineSeparator.get` | `{ path? }` | `{ path, separator, label, found, mixed }`: the separator of a file (the active editor's by default) as the status bar shows it: `lf`, `crlf` or `cr` from its first line break, else (`found: false`) the one new files get; `mixed` when the file uses more than one |
| `file.lineSeparator.set` | `{ separator, paths? }` | `{ changed, skipped }`: writes every line break of the files with `separator`: the active editor's file by default, or the given files and folders of the project (a folder: its text files recursively, without `.git` and excluded folders inside it). An open file changes as one undo step and is saved at once; every file written gets a Local History version labelled `lineSeparators`. `skipped` lists files that are not UTF-8 text |

The keymap actions `file.lineSeparator.set/crlf`, `/lf` and `/cr` are the menu's items; from the
menu they act on the Project tool window's selection when it has the keyboard. The settings'
`lineSeparator` (`set_settings`) is Code Style → General → Line separator.

## EditorConfig

The project's `.editorconfig` files set the code style of the files they match, above the project's
`.idea` code style and the IDE's ([ADR-0100](adr/0100-editorconfig-code-style.md), the guide's
[EditorConfig](guide/09-settings.md#editorconfig)).

| Command | Parameters | Result |
|---|---|---|
| `project.editorConfig.get` | `{ path? }` | `{ path, enabled, files, properties, style, codeStyle }` for a file of the project (the active editor's by default): `files` that have a matching section, the nearest first; `properties` by name (`unset` ones left out); `style`, what they set (`useTabs`, `indent` as `{ "columns": n }` or `"tabSize"`, `tabSize`, `continuationIndent`, `rightMargin` as `{ "column": n }` or `"off"`, `visualGuides`; `null` is left to the project and the IDE); `codeStyle`, the style the file is edited with (`useTabs`, `tabSize`, `indent`, `continuationIndent`, `rightMargin`, `visualGuides`) |

| `project.editorConfig.open` | `{ path? }` | Opens the nearest `.editorconfig` with a section matching the file (the active editor's by default) and answers its path: the indent widget's Open .editorconfig |
| `project.editorConfig.list` | `{}` | `[{ path, relative, root, problems }]`, the project's `.editorconfig` files: Show Files Related to Project… |
| `project.editorConfig.create` | `{ folder?, stubs?: { root, standard, intellij } }` | Writes `.editorconfig` in the folder (the root by default) with the chosen stubs under `[*]`, each set to what the folder's files get now, opens it and answers its path; code `editorConfigExists` when there is one |
| `project.editorConfig.export` | `{ path?, overwrite? }` | `{ path, text }`: the code style as last applied (the IDE's with the project's `.idea` style on top) written as an `.editorconfig` (`<project>/.editorconfig` by default); code `editorConfigExists` unless `overwrite` |
| `project.structure.get` | `{}` | `{ root, contentRoots, excluded, resourceRoots, excludePatterns, editable, revision }`: what Settings → Project Structure shows, from the module files (`*.iml`) (ADR-0159) |
| `project.directory.markResourceRoot` | `{ path }` | `{ changed, files, excluded, resourceRoots, excludePatterns, revision }`: Mark Directory As → Resource Root, written into the module file that holds the folder (`<sourceFolder type="java-resource">`); an exclusion or another root mark of the folder goes |
| `project.directory.unmarkResourceRoot` | `{ path }` | As `markResourceRoot`: Unmark as Resource Root |
| `project.structure.setExcludePatterns` | `{ patterns }` | As `markResourceRoot`: replaces the Exclude files name patterns (`*` and `?`, no `/`; an entry may hold several separated by `;`), one `<excludePattern>` each in the content root's module file; the index and the searches follow at once |
| `project.structure.project` | `{}` | `{ name, root, jvm, jdk?, jdks, sdk?, buildLanguageLevel?, buildFile?, compilerOutput: { path?, testPath?, origin } }`: the Project section; `origin` is `build`, `convention`, `temporary` or `unknown`; set the JDK and level with `toolchain.jdk.choose` and `toolchain.jdk.languageLevel` (ADR-0199) |
| `project.structure.modules` | `{}` | `{ modules, revision }`: the Modules section, the model's modules with `file` (the build file), `folders` (`{ path, kind }`, kind `sources`, `testSources`, `resources`, `testResources` or `excluded`), `output`, `testOutput`, `assumed` (conventions, not read from the build), `languageVersion` and `dependencies` with `scope` |
| `project.structure.libraries` | `{}` | `{ libraries, revision }`: the Libraries section, each library of `project.libraries.list` (dependencies only) with `usedBy`, the modules that declare it |
| `project.structure.sdks` | `{}` | `{ jdks, others }`: the SDKs section, the JDKs of `toolchain.jdk.list` and the other languages' SDKs (`{ name, version, home }`) |
| `project.structure.problems` | `{}` | `{ problems: [{ section, message, file?, warning }] }`: the Problems section; `section` is `project`, `modules`, `libraries` or `sdks` |
| `project.structure.openBuildFile` | `{ module? }` | `{ path }`: opens the build file of a module (by name; the root module's when absent) in the editor |
| `project.structure.artifacts` | `{}` | `{ artifacts, suggestions, jars }`: the Artifacts section; `artifacts` are kept in `.zen/artifacts.json`, each `{ name, kind, modules, mainClass, libraries, output, task }` (`kind` `jar` or `build`, `libraries` `inside` or `nextTo`); `suggestions` offer the build's packaging first for Maven and Gradle, then a jar of the modules; `jars` is where each jar is written (ADR-0201) |
| `project.artifact.save` | `{ artifact, previous? }` | The Artifacts section: adds or changes an artifact, `previous` naming the one it replaces |
| `project.artifact.delete` | `{ name }` | The Artifacts section: removes a kept artifact; its files stay |
| `build.artifacts.build` | `{ names, waitMs? }` | `{ runId?, summary?, jars, removed }`: compiles the artifacts' modules through their build (or javac) in a run of the Run tool window and writes each jar with its manifest; a build artifact runs its phase or task; every listed artifact when `names` is empty |
| `build.artifacts.rebuild` | `{ names, waitMs? }` | As `build.artifacts.build`, after removing the jars and compiling from clean |
| `build.artifacts.clean` | `{ names, waitMs? }` | As `build.artifacts.build`: removes the jars and the libraries copied next to them at once (`removed`); a build artifact runs the build's `clean` |

`project.editorConfig.get` also answers `save`: `trimTrailingWhitespace` and `insertFinalNewline`
(which win over Editor → General → On Save, `true` and `false`), `endOfLine` (a modified file is
written with it on save) and `charset` (`latin1`, `utf-8`, `utf-8-bom`, `utf-16be`, `utf-16le`: the
encoding the file is opened and saved in; `file.document.save` fails, writing nothing, on a
character it cannot hold, naming it with its line and column) ([ADR-0107](adr/0107-editorconfig-save-properties-and-files.md)).
The keymap actions `view.editorConfig.new` (`{ folder? }`: the New EditorConfig File dialog),
`view.editorConfig.showFiles` and `view.editorConfig.disable` (Enable EditorConfig support off, saved)
run with `run_action`.

Lines of an `.editorconfig` file that cannot be read are in `project.settings.get`'s `problems`
(`Line <n>: ...`), and the files in its `sources`. The settings' `editorConfig` (`set_settings`,
on by default) is Code Style → General → Enable EditorConfig support; `false` ignores every
`.editorconfig` file, `end_of_line` included.

## Recent files, recent locations and the file structure

The navigation popups are commands an agent runs without the popups (`zen_workbench::navigate`,
`zen_workbench::history`):

- **`navigate.recent.files { editedOnly? }`** returns the files shown last, most recent first; with
  `editedOnly`, only the files edited while the project is open (Recent Files' Show edited only).
- **`navigate.recent.remove { path }`** is Delete from Recent Files (⌦, ⌘⌫ or ⌫ in the Switcher and
  Recent Files): the file leaves the recent files and its tabs close in every editor group. Unsaved
  changes are saved first, as Save All saves them; a file that cannot be saved stays, with its tabs
  and changes, and the error says why. Answers `{ listed, closed, saved }`.
- **`navigate.recent.locations { editedOnly?, query? }`** returns the places of Recent Locations,
  newest first: `{ path, relative, offset, line, time, edited, breadcrumbs, snippet }`, `time` in
  seconds since 1970, `breadcrumbs` the names of the declarations around the place, `snippet` the
  five lines of code around it as `{ line, text }`. A place is kept when navigation leaves or reaches
  it (as Back records it) and when the text is edited there; one per line, 25 of each kind.
  `editedOnly` keeps the edited places; `query` keeps those whose code, file name or breadcrumbs
  contain it, ignoring case. Go to one with `navigate.place.goTo { path, offset }`.
- **`navigate.recent.removeLocation { path, line }`** forgets the places on that line: Recent
  Locations, Back, Forward and the edit places no longer have them.
- **`navigate.history.back`** and **`navigate.history.forward`** (⌘[ ⌘]) go where Back and Forward
  go and return the place reached, or `null`; `navigate.history.list` gives `{ back, forward }`,
  the next place last in each; `navigate.history.clear` forgets them, Recent Locations and the edit
  places, as opening a project does.
- **`navigate.edit.last`** (Last Edit Location, ⇧⌘⌫) goes to the latest edit place, and run again
  to the one before; **`navigate.edit.next`** (Next Edit Location) goes back toward the latest.
  Both return the place reached `{ path, offset, line }`, or `null` when the only places left were in
  deleted files (they are dropped); a closed file opens again. **`navigate.edit.list`** gives
  `{ places, current }`: the edit places, the latest last, one per line for a run of edits and
  moved with later edits, and the index of the place gone to last (`null` since the last edit).
  `find_action` gives the two as `available` exactly when their menu items are enabled.
- **`editor.split.openRight { path }`** opens a file in a new editor group on the right (Open in
  Right Split, ⇧⏎ in Recent Files and in the Project tool window).
- **`editor.structure.get { path?, sort?, fields?, query? }`** is the File Structure popup's data, as
  the Structure tool window's; `query` keeps the symbols whose names have its letters in order
  (camel humps count) and the declarations that hold them, as the popup narrows down on typing.

`run_action` with `view.projects.popup` opens the toolbar's Projects popup (the project widget's
menu), or closes it when run again without parameters. `params` may hold `open` (`true` keeps it
open, `false` closes it) and `filter`, the text typed into it; the answer's `result` is
`{ open, filter, projects }`, the projects it shows in order, each `{ name, path, branch, section
("open" or "recent"), current, selected }`. While it is open, `press_keys` types into its filter
(`{ "text": "ишд" }` finds billing-service through the Russian layout's keys), `↑`/`↓` move, `⏎`
opens the selected project and `⎋` clears the filter, then closes. `get_window_state` names it
`projects`.

`run_action` with `view.navigationBar.jump` (Jump to Navigation Bar, ⌘↑ or ⌥Home) gives the
navigation bar the keyboard with its last segment selected, over the editor when View → Appearance →
Navigation Bar is Don't Show or a mode hides the status bar. `params` may hold `segment` (selects
one, counted from 0), `open` (`true` opens the selected segment's popup, `false` closes it), `filter`
(the text typed into the popup) and `close` (`true` leaves the bar); a segment without children (a
file without its declarations shown, a declaration with nothing inside) refuses `open`. The
answer's `result` is `{ placement, showMembers, active, floating, segments, selected, popup }`: each
segment and popup entry `{ name, kind ("project", "folder", "file" or "symbol"), path, symbolKind?,
offset?, line?, hasChildren }`, the popup `{ segment, filter, status, items, selected }` or `null`.
While the bar has the keyboard, `press_keys` with `←`/`→` moves between segments, `↓` or `⏎` opens
the selected one, typed text filters the popup, `↑`/`↓` move in it, `⏎` opens a file, goes into a
folder (its popup opens at once) or goes to a declaration, and `⎋` returns to the editor.
`get_window_state` names the bar `navigationBar` and its popup `navigationBarPopup`.
`view.navigationBar.top`, `view.navigationBar.statusBar`, `view.navigationBar.hide` and
`view.navigationBar.members.toggle` set the placement and Show Members, answering
`{ navigationBar: { placement, showMembers } }`. The session commands
`navigate.navigationBar.get { path?, members?, waitMs? }` (the segments of the active file, or of
the file or folder given) and `navigate.navigationBar.children { path, offset?, waitMs? }` (a
folder's entries, a file's declarations, or with `offset` those inside a declaration) read the same
without a window.

`run_action` with `view.recentFiles.open` (⌘E), `view.recentLocations.open` (⇧⌘E) and
`view.fileStructure.open` (⌘F12) opens the popups and answers with what they list; the first two
take `{ editedOnly }`, and running them again while open toggles Show edited only, as the shortcut
does. `get_window_state` names them `recentFiles`, `recentLocations` and `fileStructure`.

`run_action` with `view.switcher.open` (⌃⇥ and ⌃⇧⇥) opens the Switcher and answers with the files
it lists, the current one first; `{ "backward": true }` moves up, as ⌃⇧⇥ does. Run by an agent it
stays open, as `switcher` in `get_window_state`, until `press_keys` gives `⏎` (open the selection)
or `⎋`; `⇥`, `⇧⇥`, `↑` and `↓` move, `←` and `→` switch between the tool windows and the files, `⌦`
or `⌘⌫` deletes the selected file (`navigate.recent.remove`). Recent Files takes the same keys. Opened by
its shortcut it lives while ⌃ is held: `press_keys` with `⌃⇥` is a quick tap, which goes back to
the previous file without showing the popup.

`run_action` with `view.selectIn.open` (⌥F1) opens Select In for the file in the editor, or for
`params.path`, and answers `{ path, targets }`: the places offered, in the popup's order, from
`projectView` (the file is in the project or one of its libraries), `structure` (a file),
`commit` (the file has changes to commit), `finder` and `terminal`. While it is open, `press_keys`
with a digit or `⏎` picks one and `⎋` closes it; `get_window_state` names it `selectIn`.
`view.selectIn.target { target, path?, dryRun? }` goes to one place at once, as picking it does, and
closes the popup:
`projectView` selects the file in the tree (under External Libraries for a library's file) and gives
the tree the keyboard, `structure` shows the Structure tool window at the symbol at the caret,
`commit` selects the change in the Commit tool window (refused when the file has none), `finder`
runs `file.fs.reveal` (`dryRun` only checks) and `terminal` runs `view.project.openInTerminal`. The
answer is `{ target, path }`, with `terminal` (the new tab) for `terminal`.

## Bookmarks

Bookmarks are commands of the project window (`zen_workbench::bookmarks`). Lines count from 1, as
every place an agent sees, also in the `view.bookmark.*` actions;
`path` and `line` default to the caret's line of the active editor. A bookmark of a whole file or
folder ([ADR-0121](adr/0121-file-and-folder-bookmarks.md)) is named by `{ path, whole: true }` in
every command below that takes a place, and listed without `line` (with `folder: true` for a
folder). A mnemonic is a digit `0`–`9`
or a letter `A`–`Z` (a small letter counts as its capital); each is used by one bookmark at most.

| Command | Parameters | Result |
|---|---|---|
| `navigate.bookmark.toggle` | `{ path?, line? }` | `{ bookmark }`, `null` when one was removed (Toggle Bookmark, F3) |
| `navigate.bookmark.toggleMnemonic` | `{ mnemonic, path?, line? }` | `{ bookmark }`; removes the line's bookmark when it has the mnemonic, else gives the line a bookmark with it, taking it from the line that had it, without asking (⌃⇧ and a digit) |
| `navigate.bookmark.setMnemonic` | `{ mnemonic, description?, replace?, path?, line? }` | `{ bookmark }`; a mnemonic another line has fails with the `code` `mnemonicTaken` and that `{ path, line }` in `data`, unless `replace` moves it here |
| `navigate.bookmark.removeMnemonic` | `{ path?, line? }` | `{ bookmark }`, now anonymous |
| `navigate.bookmark.rename` | `{ description, path?, line? }` | `{ bookmark }`; the description the gutter icon shows on hover, empty clears it |
| `navigate.bookmark.moveToList` | `{ list?, index?, path?, line? }` | `{ bookmark }`; puts it in another list, created by its name, or in the default one without a name, at `index` from 0 in it or last (what a drag in the Bookmarks tool window does) |
| `navigate.bookmark.moveUp`, `navigate.bookmark.moveDown` | `{ path?, line? }` | `{ bookmark }`; one place up or down in its list (Move Up, Move Down); the first or last one stays |
| `navigate.bookmark.remove` | `{ path?, line? }` | Nothing |
| `navigate.bookmark.list` | `{ path? }` | `{ bookmarks, defaultList, lists, groups, replaceWithoutAsking, groupByFile }`: `[{ path, line, mnemonic?, description?, list? }]` by file and line (`list` absent in the default list), the default list's name (at first the project folder's), the other lists in their order, `groups` — every list in the tool window's order, the default first, as `{ name, isDefault, bookmarks }` with its bookmarks in their order — and the two options |
| `navigate.bookmark.options` | `{ replaceWithoutAsking?, groupByFile? }` | `{ replaceWithoutAsking, groupByFile }`: the Bookmarks tool window's ⋮ options; `replaceWithoutAsking` is Ask Before Rewriting Mnemonic turned off (the question's Don't ask again) |
| `navigate.bookmark.open` | `{ path, line? }` | `{ path, offset, line }`: opens a bookmark's or breakpoint's line, or without `line` a bookmarked file (Jump to Source, Enter in the tool window and the popup); recorded for Back. A folder is refused with the code `folderBookmark` and its `path` in `data` (the window selects it in the Project tool window; agents use `view.project.select`) |
| `navigate.bookmarkList.create` | `{ name }` | The name; an empty list after the others (Create Bookmarks List); a name in use is refused |
| `navigate.bookmarkList.rename` | `{ name, newName }` | The new name; the list keeps its bookmarks and place |
| `navigate.bookmarkList.setDefault` | `{ name }` | The name; Mark List as Default: the list goes to the top and new bookmarks go to it |
| `navigate.bookmarkList.delete` | `{ name }` | How many bookmarks it took from the code (Delete Bookmark List); with no list left, the next bookmark makes one named after the project |
| `navigate.bookmark.toggleFiles` | `{ paths }` | `{ bookmarks }`: Toggle Bookmark on the files and folders (the Project tool window's selection, F3): removes their bookmarks when every one has one (`bookmarks` empty), otherwise bookmarks each without one in the default list; their line bookmarks stay |
| `navigate.bookmark.removeFiles` | `{ paths }` | How many of their own bookmarks were removed (the Project tool window's Bookmarks › Delete) |
| `navigate.bookmarkList.fromOpenTabs` | `{ name }` | `{ name, isDefault, bookmarks }`: Bookmark Open Tabs…, a new list with a bookmark of every open tab's file in tab order, those bookmarked already moving into it; a name in use is refused |
| `navigate.bookmark.goTo` | `{ mnemonic }` | `{ path, offset, line }`: where it went; the place left is recorded for Back (⌃ and a digit) |
| `navigate.bookmark.next`, `navigate.bookmark.previous` | none | `{ path, offset, line }`: the bookmark after or before the caret, by file and line, going round; recorded for Back |

Bookmarks move with the edits of open files, a deleted line taking its bookmark along; a change
made by another program to an open file moves them by the diff of its text. They follow a file or
folder renamed or moved in the IDE (`file.fs.rename`, `project.file.paste` after a cut) and go with
a deleted file, and are saved per project in `workspace.json` in the IDE's data folder (`bookmarks.lines`
by list and order, each of a list other than the default naming it, `bookmarks.files` for files and
folders with `index`, their place in their list, apart so that older builds still read `lines`, `bookmarks.lists` in their order with the default first,
`bookmarks.replaceWithoutAsking`, `bookmarks.groupByFile`), never in the project. The window's own actions open the
dialogs: `view.bookmark.mnemonic` (Add Mnemonic Bookmark…, ⌥F3), `view.bookmark.goToMnemonic` (Go
to Mnemonic…, ⌥⌘F3), `view.bookmark.rename` (Rename Bookmark…) and `view.bookmark.moveToList` (Add
Bookmark to Another List); all but Go to Mnemonic take `{ path?, line?, whole? }`, the caret's line by
default. `view.bookmark.openTabs` (Bookmark Open Tabs…, in an editor tab's Bookmarks › menu) asks
for the new list's name (`bookmarkOpenTabs`). While the Project tool window has the keyboard (after
`view.project.select`), F3 and ⌥F3 (`press_keys`) act on its selection: Toggle Bookmark on every
selected row, and the mnemonic chooser for the last one. Its context menu
(`view.project.contextMenu`) and an editor tab's have a Bookmarks › submenu with Toggle Bookmark,
Toggle Bookmark Mnemonic… and Delete. `get_window_state` names them `bookmarkMnemonic`, `goToMnemonic`, `bookmarkRename`
and `bookmarkList`, and the question before a taken mnemonic moves `bookmarkReplace`.

The **Bookmarks tool window** (`view.toolWindow.bookmarks`, ⌘2; id `bookmarks`, at the left bottom)
shows `groups` and, as a list of its own, the breakpoints (`debug.breakpoint.list`). Its actions are
the commands above; `view.bookmarks.next` and `view.bookmarks.previous` (⌃⌥↓ and ⌃⌥↑, also ⌥⌘↓ and
⌥⌘↑, while the window has the keyboard) select and open the next or previous bookmark or breakpoint
of its tree, going round, and answer where they went. Its dialogs are `bookmarkListName` (Create
Bookmarks List, Rename…) and `bookmarkListDelete` (the question before Delete Bookmark List).
**Show Line Bookmarks…** (`view.bookmark.show`, ⌘F3; `lineBookmarks`, run again to close) answers
`{ rows }`: `[list]` for a list, then `M file:line description` for each bookmark; `press_keys`
with a mnemonic's `text` jumps to it, `↓` and `⏎` open the one selected. With the setting
`bookmarksPopupLineOnly` off (Advanced Settings, on by default) the breakpoints are listed too. While the
chooser is open, `press_keys` with `text` chooses a character (`{ "text": "k" }`), more text goes
into the description and `⏎` sets it.

## Find and replace

The search bar's commands (`editor.find.*`) work on the active file; Replace in Files
(`search.text.replaceAll`) uses the same replacement engine (`zen_text::search::replacements`) on
the project's files. The query (`{ text, matchCase, wholeWords, regex, context, preserveCase }`) is
shared by both; `context` is the Filter (`anywhere` by default, `inComments`, `inStringLiterals`,
`exceptComments`, `exceptStringLiterals`, `exceptCommentsAndStringLiterals`), judged by the
highlighting token a match starts in, and only the matches it counts are
replaced, previewed and stepped through.

- **The replacement.** With `regex`, `$1`, `${name}` and `$0` are filled in from the match, and
  `\U…\E` and `\L…\E` turn what is between them (groups included) into capitals or small letters,
  `\u` and `\l` the next character; `\\` is a backslash and `\$` a dollar. Without `regex` the
  replacement is taken as written.
- **Preserve Case** (`preserveCase`) gives each replacement the case of its match: all capitals
  (`FOOBAR` → `BAZQUX`), all small letters (`foobar` → `bazqux`), a capital and then small letters
  (`User` → `Account` for `ACCOUNT` or `account`) or a small letter and then capitals (`uSER` →
  `aCCOUNT`) when the replacement is in one case (a mixed one keeps its own case after the first
  letter), or, otherwise, the first letter in the case of the match's first letter
  (`FooBar` → `BazQux`, `fooBar` → `bazQux`). Only letters count. The search then ignores case, so
  Preserve Case and Match Case exclude each other in the UI.
- **`editor.find.exclude`** (the Exclude button) skips the selected match: Replace and Replace All
  leave it alone and the next match that is not skipped is selected; on a skipped match it takes it
  back (the button reads Include). `editor.find.info` reports the skipped matches of the active file
  as `excluded` (character ranges) and `currentExcluded`. Each file keeps its own; they move with
  the edits, undo included, as folds do, and a new query or closing the bar forgets them.
- **`editor.find.preview { fromLine?, toLine? }`** returns
  `{ matches: [{ start, end, replacement, excluded }], error }`: what each match of the active file
  that starts on those lines (one-based, the whole file by default) becomes. While the replace row is
  open and Regex is on, the editor shows each replacement as a hint right after its match (not for
  skipped matches), listed for the shown lines and 50 around them.
- **`editor.find.open { replace?, keepQuery? }`** (⌘F, ⌘R) takes a selection on one line as the
  query, escaped (`zen_text::search::escape`) while Regex is on; a selection of several lines turns on In Selection and leaves the query; a caret
  alone leaves the query as it was (empty at first: the word at the caret is not
  taken). `keepQuery` (the bar's replace toggle) changes neither.
- **In Selection.** `editor.find.searchInSelection { on? }` (⌃⌥E, the Filter's first item) keeps the
  search to the text selected when it is turned on: each file keeps that range and moves it with the
  edits; counting, stepping, Replace All, Select All and the preview keep to it. `editor.find.info`
  reports it as `inSelection` and `scope`; closing the bar ends it.
- **`editor.find.scrollToResults { on? }`** (⋮ → Scroll to Results During Typing, on by default):
  while it is off, `editor.find.set` counts the matches without moving the selection. It is kept for
  the window, not saved.
- **`editor.find.openInFindWindow`** (⌥F7 in the search bar, ⋮ → Open Results in Find Window) lists
  the active file's matches on a tab "Occurrences of '…'" of the Find tool window (`kind: "file"`,
  grouped and filtered as the other tabs); it answers, and `navigate.usages.list` reads, the usages
  state with `occurrences: true` and the tab's `tab`.
- **`editor.find.nextWordAtCaret`** and **`editor.find.previousWordAtCaret`** (Edit → Find → Next /
  Previous Occurrence of the Word at Caret) make the selected text or the word at the caret the query
  (whole words, match case) and select its next or previous occurrence, wrapping around.
- **The history.** `search.findHistory.list` returns `{ queries, replacements, masks }`, newest
  first, at most 50 each, shared by the search bar and Find in Files and kept in `find-history.json`
  in the data folder. Typing keeps nothing: the bar adds its query when it steps, selects all,
  replaces, opens its results in the Find window or closes, Find in Files (with its file mask) when
  it opens a match, replaces, opens its results in the Find window or closes
  (`search.findHistory.add { query?, replacement?, mask? }`); `search.findHistory.clear` forgets
  it. ⌥↓ in either field, or the lens button at its left, lists it under the field.
- **Unselect Occurrence** (`editor.selection.unselectOccurrence`, ⌃⇧G) drops the selection that Add
  Selection for Next Occurrence (⌃G) added last, also when it wrapped around to the top of the file,
  and makes the one added before it primary; after any other change of the selections it drops the
  primary one. An edit forgets the order.

## Camel humps

Settings → Editor → General → Smart Keys → Use "CamelHumps" words (`camelHumps`, off by default)
makes `editor.caret.move` with `wordLeft` and `wordRight` (⌥← ⌥→, with `extend` ⌥⇧← ⌥⇧→) and
`editor.edit.deleteWordBackward` and `editor.edit.deleteWordForward` (⌥⌫ ⌥⌦) stop at camel humps:
`Bar` in `fooBarBaz`, `Server` in `HTTPServer`, `bar` in `foo_bar` (the session setting
`editor.smartKeys.camelHumps`, `zen_workbench::editing::camel_humps`). Each of these commands takes
`camelHumps: true | false` to go one way or the other whatever the setting; the movements `humpLeft`
and `humpRight` always stop at humps.

`editor.selection.word { at?, camelHumps? }` selects the word at a place (an offset or `{ line,
column }`, the primary caret by default) as a double click does: the identifier, or only its hump
when `camelHumps` and the double-click option of CamelHumps words
(`camelHumpsOnDoubleClick`, on by default, `editor.smartKeys.camelHumpsOnDoubleClick`) are both on.
A caret on a blank stays a caret. The double click in the editor uses the same rule
(`zen_editor::double_click_word`); `zen_editor::hump_at` gives the hump for Extend Selection.

## Clipboard history

Paste from History (⇧⌘V, `view.pasteFromHistory.open`) keeps the texts copied and cut in the IDE,
newest first, in memory and only while the IDE runs (`zen_workbench::clipboard_history`). The window
records what it puts on the clipboard itself (egui's `CopyText` output) through
`editor.clipboardHistory.add`; the system clipboard is never read for it, so other programs' copies
never get in. The windows of one IDE share one history; `clipboardLimit` (1–100, 20 by default)
caps it, and a smaller limit drops the oldest entries as soon as the settings are applied. A text
over 1 MiB is not kept. The history is never written anywhere: not to `settings.json`, not to
`workspace.json`, not to a log; `get_window_state` names the popup (`pasteFromHistory`) and nothing
of its entries. Copies from the terminal and the IDE's other text fields count like the editor's;
the IDE has no password fields (git runs never ask for one, `zen_vcs::git`), and egui never copies
from one, so no field's secret text gets in by design.

- **`editor.clipboardHistory.list`** returns `[{ index, preview, lines, chars }]`, `index` 0 for the
  newest. Copied text often holds secrets (tokens, passwords), so an agent never gets an entry
  whole: `preview` is its first line with text in it, trimmed and cut to 60 characters.
- **`editor.clipboardHistory.paste { index? | indices? }`** pastes one entry (the newest by default),
  or several joined by line breaks, at every caret of the active file, as `editor.edit.paste` does,
  and makes it the newest entry; it returns the selections. It leaves the system clipboard alone
  (the popup puts what it pastes there). What is pasted is in the file, and
  so readable like any text there: the previews only keep entries from being read by accident.
- **`run_action { action: "view.pasteFromHistory.open" }`** opens the popup and answers with the
  same previews as `result`.
- **`editor.clipboardHistory.add { text }`** only adds: it answers whether the text was kept (an
  empty text or one over 1 MiB is not), never the entries.
- **`.remove { index }`** and **`.clear`** change the history and return it as previews.

## Read-only files

`file.document.toggleReadOnly { path?, readOnly? }` turns a file's write permission on disk off
(`readOnly: true`) or on (`false`), or the other way when `readOnly` is absent; `path` defaults to
the active document's file (relative paths are the project's). Unsaved changes are saved before the
file is made read-only (the command fails, and the file stays writable, when they cannot be);
making it writable again gives back the write bits it had, or the owner's alone when the IDE did not
make it read-only. `file.document.readOnlyState { path? }` reads the state without changing it.
Both answer `{ path, readOnly, reason? }`: `readOnly` is the permission on disk (the status bar's
lock), `reason` why the open document cannot be edited, when it cannot (the file is read-only, or
its encoding is not UTF-8). Edits to a read-only file's document change nothing; the window then
posts a warning notification whose `makeWritable` action runs `file.document.toggleReadOnly` with
`readOnly: false` (`ui.notifications.list`, `ui.notifications.action`); it is marked read once the
file is writable, however that happened. `search.text.replaceAll`, `editor.text.replace` and
refactorings refuse a read-only file, open or not.

## Caffeine

Caffeine keeps the computer awake from the IDE (`zen_workbench::caffeine`, guide: Caffeine). One
Caffeine serves every window of the IDE; it runs `/usr/bin/caffeinate <flags> [-t <seconds>] -w
<ide pid>` as a child of the IDE (`systemd-inhibit ... tail --pid=<ide pid>` on Linux), never
anything through `unsafe` code, so nothing stays awake once the IDE has ended. Exiting the IDE (and
the snapshot's quit) stops it at once.

- **`caffeine.status.get`** returns `{ available, backend?, on, awake, what, length?, secondsLeft?,
  while?, busy, paused?, ended?, battery?, pid?, error?, maximum }`: `awake` while the program runs;
  `maximum` while it is on with every option until turned off; `length`
  is `{kind: indefinitely | minutes | while}`; `busy` is what runs now of `run`, `debug`, `task`,
  `indexing` and `agent`; `paused` says why it is on but not holding (`onBattery`, `lowBattery`,
  `waiting`); `ended` why it last turned off by itself (`expired`, `battery`); `battery`
  `{ percent?, onBattery, charging }` only while a battery rule needs it (`pmset -g batt` once a
  minute).
- **`caffeine.awake.start { what?, minutes?, while?, maximum? }`** turns it on: `what` from `idle` (-i),
  `display` (-d), `disk` (-m), `system` (-s) and `user` (-u); `minutes` 1–10080, 0 for until stopped;
  `while` a list of the activities above, only while one of them runs (an empty list takes the
  settings'); `maximum: true` is every option until stopped, whatever the other three say. What is
  left out comes from the settings `caffeine`; the battery rules always do. It fails where there is no
  program ("not available"). A start with a new time while it is on starts the program again with
  the new `-t`, and a timer of its own ends it at the deadline, whether or not the IDE draws a
  frame, and wakes the IDE so that its windows show it off; `caffeine.status.get` after the
  deadline answers `on: false, ended: expired`. The time is counted on the monotonic clock
  (`Instant`), which stands still while the computer sleeps, like `caffeinate -t`'s own: a time
  that spans the computer's sleep ends that much later by the wall clock.
- **`caffeine.awake.stop`** and **`caffeine.awake.toggle`** (Turn On and Turn Off in the tool window) return the status.
- **`caffeine.awake.maximum`** is the tool window's **Maximum Caffeine** button: on with every option
  until turned off, or off when it is on at maximum already (`maximum` in the status); returns the
  status. The settings, the battery rules among them, are left as they are.
- An agent counts as connected (`agent`) for two minutes after its last tool call, and while a stdio
  bridge holds a lease on the IDE (ADR-0085).
- The settings are `caffeine` in `get_settings`/`set_settings`: `what`, `length`, `while`,
  `customMinutes`, `offOnBattery`, `lowBattery`, `lowBatteryLevel` (5–95), `resumeOnCharge`,
  `startOnLaunch`; a change of `what` or `while` applies to Caffeine that is on.
- The window action **`view.toolWindow.caffeine`** shows or hides the Caffeine tool window (right top,
  under the Git workspace button; `get_window_state` then has `"right": "caffeine"`).
  `get_window_state` gives the coffee drinker's state as `caffeine`: `asleep` (off), `waiting`
  (paused), `steady` (keeping the system awake), `wide` (the display too) or `wired` (every "keep
  awake" option on); the settings page is `settings:caffeine`.

## Database

The Database workspace and its foundations ([ADR-0163](adr/0163-workspaces-full-tools-in-the-editor-area.md), specification:
[database-workspace.md](database-workspace.md)). This section grows with each step of the plan;
today it holds the workspace host every workspace uses, the Database workspace's consoles, the
connection registry and the connection lifecycle.

A refusal's message is written for whoever called (`database::for_caller`): a person reads the
controls to use ("choose one in the connection list on the console's toolbar"), an agent the
command (`db.console.chooseConnection`). The code and data are the same for both, so an agent acts
on the code; this holds for `noConnection` (no connection at all, or a console without one),
`noObjectTab`, `unknownDetection` and `noBinding`.

### Workspaces

A workspace expands over the editor area and collapses back without closing anything. It owns
editor groups outside the main split tree: `editor.split.list` reports them under
`workspaces: [{ workspace, root, focused }]`, `root` a split tree of their own as the main one's
(`{ kind: "group", id, tabs, active }` or `{ kind: "split", direction, ratio, first, second }`, ids
from 2³² up, never renumbered) and `focused` the one that last had the keyboard, with `mainFocused`,
the main group that last had the keyboard. A workspace is **expanded exactly when one of its groups
is the focused one**: focusing any main group (`editor.split.focus`, opening a code file) collapses
it. New tabs never go into a workspace's group unless `file.document.open { path, group }` names
it; a file already shown there comes to front there.

**Splits inside a workspace** (ADR-0189). While a workspace is expanded, `editor.split.right`,
`.down`, `.unsplit`, `.unsplitAll`, `.next`, `.previous` and `.resize` act on its area: Split Right
and Split Down make a new group of the workspace beside the focused one, showing the tab (a text tab
stays in the old group too unless `move`; a tab the feature draws always moves, and is refused when
it is the only tab of its group), and Goto Next Splitter goes round the workspace's groups. A group
of a split workspace left without tabs goes. `workspace.tab.split` does the same for a tab named by
id. Each inner tab carries its `group`; `workspace.host.state` lists the groups and the tab each
shows under `panes`, and `active` is the tab of the group that has the keyboard.

**Scripts between a workspace and the editor.** A project's file a workspace takes moves between
the main editor and that workspace (the Database workspace takes the project's `.sql` scripts):
`editor.split.moveTab { from, index, to }` with `to` a group of the workspace, or `{ from, index,
workspace }` for the workspace's group that last had the keyboard (it expands), and
`workspace.tab.move { workspace, tab, group }` with `group` a main group (the workspace collapses
and the script shows in the code editor). Anything else between a workspace and the main editor, or
between two workspaces, is refused with code `notMovable` and the reason: a console stays in its
workspace (it lives in the IDE's data folder), a tab the feature draws stays, a code file does not
go into a workspace that does not take it.

| Command | Parameters | Answers |
|---|---|---|
| `workspace.host.list` | none | `{ workspaces: [{ id, title, icon, expanded, open, tabs }], expanded }` |
| `workspace.host.state` | `id`, `limit?` (50) | `{ id, title, icon, expanded, open, group, panes: [{ group, active }], returnTo, active, tabs: [{ id, kind, title, path?, closable, data?, group }], total, truncated }` |
| `workspace.host.expand` | `id` | the state; the workspace's group takes the keyboard |
| `workspace.host.collapse` | `id?` (the expanded one) | the list; the main group in `returnTo` takes the keyboard |
| `workspace.host.toggle` | `id` | the state; what the workspace's stripe button runs |
| `workspace.tab.open` | `workspace`, `path?`, `kind?`, `title?`, `id?`, `closable?`, `data?` | the tab; with `path` a text tab (a document of the workspace's group), else a tab of `kind` its feature draws; a tab with the same file or `id` comes to front; a file the workspace does not host (the Database workspace hosts `.sql` files only) is refused with `notHosted`: it opens in the editor (`file.document.open`) |
| `workspace.host.close` | `id` | the list; collapses the workspace and takes its tab out of the editor strip (what its × runs); its inner tabs stay |
| `workspace.tab.open` | `workspace`, `path?`, `kind?`, `title?`, `id?`, `closable?`, `data?`, `group?` | the tab; with `path` a text tab (a document of the workspace's group that shows the file, else of `group` or the one that last had the keyboard), else a tab of `kind` its feature draws; a tab with the same file or `id` comes to front |
| `workspace.tab.select` | `workspace`, `tab` | the state; expands the workspace |
| `workspace.tab.close` | `workspace`, `tab` | the state; a text tab is saved first, an empty console (in `<data folder>/consoles`) is deleted, a tab that does not close is refused |
| `workspace.tab.move` | `workspace`, `tab`, `to?`, `group?` | the state; `to` is the tab's new place among the tabs of its group (the active tab stays); `group` moves it to another group of the workspace, which shows it, or a project script to a main group (code `notMovable` otherwise) |
| `workspace.tab.split` | `workspace`, `tab`, `direction` (`right`, `down`), `move?` | the state; a new group of the workspace shows the tab and takes the keyboard, as Split Right and Split Down do |

Kind data is the feature's small state for a tab (never
data rows, cell values or secrets). Nothing here connects anywhere.

**Open workspaces and the editor strip** (ADR-0187). A workspace is `open` from its first expansion
until `workspace.host.close`: the tab strip of the main group in `returnTo` shows its tab after the
code's tabs (a click runs `workspace.host.expand`, × `workspace.host.close`), and stays above the
host while it is expanded. Collapsing keeps it open. `workspace.json` keeps `open`.

**The Git workspace** is the workspace `git`: its pinned tab `graph` (the graph and its panels),
`diff` for a diff opened from it while it is expanded, `rebase` for the interactive rebase editor
and `conflict` for the conflict editor. `vcs.workspace.open` expands it on `graph`,
`vcs.workspace.close` closes it, `vcs.rebase.plan` and `vcs.conflict.open` expand it on their tab,
and `workspace.tab.close` on `rebase` or `conflict` is Cancel or Close. `get_window_state` answers
`editor: "gitWorkspace"`, `"diff"`, `"rebaseEditor"` or `"conflictEditor"` for its tab in front.
Its stripe button is the first on the right stripe and runs `workspace.host.toggle { id: "git" }`;
Find Action reaches it as Git Workspace (`vcs.workspace.open`).

**The stripe and Find Action.** Each registered workspace has a button at the top of the right
stripe, under the Git workspace's: lit (and "pressed" for screen readers) while it is expanded, its
tooltip the workspace's title; a click runs `workspace.host.toggle { id }`. Find Action lists
"*Title* Workspace" (`run_action { action: "workspace.host.toggle/<id>" }`), so the toggle is reached
in Distraction Free, Zen and Presentation modes, which hide the stripes. A workspace's button shows
only while its feature registers it: the Database workspace (`database`) is registered by the
built-in Database plug-in (ADR-0164, decision 1), on by default in GoGarden, RustRonin, WebWave, SqlScroll and
DevopsDaruma and off in JvmJizo (`plugins.module.enable { id: "database" }` and a restart switch it on).

**Keys and agents act on what is in front, never on code hidden beneath.** While a workspace is
expanded:

- **⌘W** (`file.document.close` without `group` and `index`) closes the active inner tab: a text tab
  as Close Tab does, a tab the feature draws through `workspace.tab.close` (answering `{ closed: [],
  saved: [], tab }`); a tab that does not close is refused with its name. With the workspace empty,
  nothing closes.
- **⇧⎋** collapses it while the workspace has the keyboard; when a tool window took the keyboard
  (a click in it, or showing it from its stripe button or shortcut) ⇧⎋ hides that tool window as
  before. **⎋ never collapses**: it cancels what it cancels in an editor.
- The **Switcher** (⌃⇥) and **Recent Files** (⌘E) list the consoles among the files and, under them,
  the workspaces' tabs that are not files; picking one, collapsed or not, expands its workspace on it.
- A console is the text in front: editor commands, `get_document_text` and the status bar work on
  it. On a tab the feature draws, every `editor.*` command that acts on the active text (all but
  `editor.split.*`, `editor.tab.*` and those given a `path`) is refused, by `dispatch_command`,
  `run_action` and keys alike, with a reason naming the tab and the workspace, as are the window
  actions that need a text file (Go to Line, File Structure, Rename…); the conflict editor in front
  is refused the same way. `get_window_state` then has `editor: "workspace"` and
  `workspace.activeTab` names the tab.

**Kept across restarts.** The project's `workspace.json` gains `workspaces: [{ id, expanded,
returnTo, tabs: [{ id, kind, title, path?, closable, data?, caret?, firstLine? }], active, panels }]`:
the tabs in order with their kind data, each console's caret (a character offset) and first shown
line, and the panel sizes the feature reports; never data rows, cell values or secrets. It is read
leniently: an entry or a tab that cannot be read drops itself, and older files load. A restart opens
the main tabs first, then each workspace's text tabs whose files still exist (with their carets),
then its other tabs from what was kept, then gives the keyboard to the expanded one. It connects
nowhere: every tab comes back disconnected (AGENTS.md rule 10). A workspace whose feature is not
registered yet is kept as it was and comes back when it registers. Workspace tabs belong to the
project: `file.project.open` in a window with another project saves that project's consoles, keeps
its workspaces' tabs in its own `workspace.json` entry and drops them, then gives back, collapsed and
without connecting, the tabs remembered for the new project. The inner tab strip stays shown in
Distraction Free, Zen and Presentation modes. Consoles are saved after a second
of quiet even when autosave is off, so a killed IDE keeps their text.

### The Database workspace and its consoles

The built-in Database plug-in registers the workspace `database` (icon `database`, title
"Database") and the `db.*` commands; while it is off, neither exists. The workspace always has the
**Overview** tab first (`{ id: "overview", kind: "overview", closable: false }`): "No connections
yet" with New Console until connections exist. A console is an SQL file in the IDE's data folder,
`<data folder>/consoles/<project key>/<connection>/console.sql` (`console_1.sql`, … for the next),
where the project key is the project folder's name and a short hash of its path, and the
connection is `local` until connections exist. A console is a text tab of the workspace: editor
commands, `get_document_text`, completion, running, Recent Files and the Switcher work on it; closing it while empty
deletes its file. The consoles folder is open to agents like scratch files: the rest of the data
folder stays hidden, and a console in front never makes other calls refuse or warn. Nothing here connects anywhere.

| Command | Parameters | Answers |
|---|---|---|
| `db.console.new` | `connection?` (the connection selected in the Database Navigator, else the one of the workspace's active tab, else `local`), `text?` | `{ path, connection, tab }`: the console created and opened as a tab of the Database workspace, which expands; a connection name with a slash, a colon or a leading dot is refused; with no connection at all it is refused with `noConnection` (add one with `db.dataSource.add` first). Two open consoles of the same file name are titled with their connection's name, `console.sql (Shop)` |
| `db.console.list` | `connection?`, `limit?` (50) | `{ folder, consoles: [{ path, connection, name, size, open }], total, truncated }`: the open project's consoles, the most recently changed first; never their text |
| `db.connection.setSecret` | `connection`, `kind` (`password`, `ssh`, `token`), `value`, `scope?` (`global`, `project`; checked against the connection's) | `{ connection, kind, scope, operation }`: the secret of an existing connection is stored in the macOS Keychain in the background, replacing the one there (`db.operation.state { operation }`: kind `storeSecret`; a denied Keychain access or an unsupported system ends it with error kind `secret` and the reason, also as a notification); never the value. An unknown connection, an empty value and a connection id with a slash are refused at once (the value never appears in the reason) |

**Secrets** ([ADR-0165](adr/0165-database-connections-types-and-secrets.md), decision 4) live only
in the macOS Keychain, as items of the service `space.ifzenelse.zen.database` whose account is
`<connection id>/<kind>`, or `<project key>/<connection id>/<kind>` for a project connection. No
command, answer, log or file ever holds one: `db.connection.setSecret` stores one and answers
without it, and an agent can never read a secret back. The IDE reads each item from the Keychain at
most once per run and remembers a denied access (a cancelled Keychain prompt) instead of asking
again, until the person retries; that read happens on the database runtime when connecting, never
on the window's thread, and so do storing and deleting (operations `storeSecret` and
`deleteSecrets`, which the commands answer at once). `db.connection.setSecret` refuses a connection that does not exist
(`unknownConnection`); a password stored with it turns on the connection's Save password, and
`hasPassword` then answers true without reading the Keychain. Removing a connection deletes its
secrets.

### Connections and connection types

Connections live in two files ([ADR-0165](adr/0165-database-connections-types-and-secrets.md)):
global ones in `<data folder>/databases/data-sources.json`, project ones in the project's
`.zen/data-sources.json`, which is shared through Git and never holds a secret. For a project
connection the user name, port, read-only, connection type and save-password are the person's own,
kept in `<data folder>/databases/<project key>/overrides.json`, unless a call says `shared: true`.
Both files are `{"version": 1, "folders": [...], "connections": [...]}`; fields this version does not
know are kept, and a file is written only when it changes. A connection's answer is its settings in
effect with `scope`, `personal` (the overrides), `hasPassword` and `state`; **no answer ever holds a
secret**. Passwords go to the secret store under `<connection id>/password` (a project connection:
`<project key>/<connection id>/password`) of the service `space.ifzenelse.zen.database` in the
macOS Keychain; the registry remembers that one was saved (`passwordSaved`), so `hasPassword` never
reads the Keychain.

The connection dialog is the window action `view.dataSource.open` (Find Action: **New Database
Connection…**; `run_action` with `params`): `{ id?, driver?, profile?, url?, name?, host?, port?, database?,
user?, folder?, type?, scope?, page?, detection? }` opens it for a new connection, or for `id` to
edit, with those fields filled in (`url` is split into the fields; `page` is `main`, `ssh`,
`advanced` or `general`; `detection` fills it from a `db.detect.list` offer, its password too, and
OK then runs `db.detect.accept`; an agent is refused one from a file hidden from it). It answers `{ "dialog": "connectionDialog" }` and shows in `get_window_state` as the
popup `connectionDialog`. OK runs `db.dataSource.add` or `.update` with the fields; Test Connection
runs `db.dataSource.test`. Agents can call those commands directly with the same fields.

| Command | Parameters | Answers |
|---|---|---|
| `db.dataSource.list` | `scope?` (`global`, `project`), `folder?`, `limit?` (50) | `{ connections, folders: [{ scope, path }], total, truncated }`, project connections first |
| `db.dataSource.get` | `id` | the connection |
| `db.dataSource.add` | `scope?` (`global`), `name`, `driver?` (`postgresql`, `mysql`, `mariadb`, `redis`, `clickhouse`), `url?`, `host?`, `port?`, `database?`, `user?`, `auth?` (`password`, `none`), `type?` (`dev`), `readOnly?`, `savePassword?` (true), `password?`, `folder?`, `tls?` (`{ mode, caFile, clientCert, clientKey, direct }`), `ssh?`, `navigator?`, `filters?`, `bootstrap?` (`{ defaultSchema, initStatements, isolation }`), `overrides?` (the type's settings for this connection), `settings?` (the driver's: `{"pooler": true}`), `shared?` | the connection (`ssh` is `{ host, user?, port?, jump? }`: a host or `~/.ssh/config` alias, and jump hosts for `ssh -J`); a `password` (or a `url`'s, also `?password=`, removed from the kept URI; `sslpassword` is never kept, in the URI or in `settings`) is stored as a secret in the background, `secretOperation` (kind `storeSecret`); without `auth`, a connection given no password gets `auth: none` when its server commonly needs none (Redis and Valkey, SQLite, ClickHouse's `default` user), else `password` (a PostgreSQL, MySQL or MariaDB URL without a password asks for it when connecting); connects nowhere |
| `db.dataSource.add` | `scope?` (`global`), `name`, `driver?` (`postgresql`, `mysql`, `mariadb`, `redis`, `clickhouse`), `profile?` (a database profile of `db.driver.list`, such as `cockroachdb`: sets the driver and, for a new connection, the port, database, TLS mode and settings not given; a `url` with a profile's scheme, `cockroachdb://`, `redshift://`, `valkey://`, or a managed service's host, `*.neon.tech`, picks it too; `""` takes it off; an unknown one is refused with code `unknownProfile`), `url?` (a file database's URL gives its path: `sqlite:///abs/shop.db` and `file:///abs/shop.db` are `/abs/shop.db`, `sqlite:data/shop.db` the relative `data/shop.db`), `host?`, `port?`, `database?`, `user?`, `auth?` (`password`, `none`), `type?` (`dev`), `readOnly?`, `savePassword?` (true), `password?`, `folder?`, `tls?` (`{ mode, caFile, clientCert, clientKey, direct }`), `ssh?`, `navigator?`, `filters?`, `bootstrap?` (`{ defaultSchema, initStatements, isolation }`), `overrides?` (the type's settings for this connection), `settings?` (the driver's: `{"pooler": true}`), `shared?` | the connection (`ssh` is `{ host, user?, port?, jump? }`: a host or `~/.ssh/config` alias, and jump hosts for `ssh -J`); a `password` (or a `url`'s, also `?password=`, removed from the kept URI; `sslpassword` is never kept, in the URI or in `settings`) is stored as a secret in the background, `secretOperation` (kind `storeSecret`); connects nowhere |
| `db.dataSource.update` | `id` and any field of `add` | the connection; absent fields stay; `settings` merge key by key (`null` removes one); an agent may not give `settings.program` or `settings.javaHome` a new value (`agentNotAllowed`, as on `add`), and an agent's change that makes it less safe (read-only off, a looser type or `overrides`) is refused with code `agentNotAllowed` (also for a read-only connection or a type that asks before changes) |
| `db.dataSource.add` | `scope?` (`global`), `name`, `driver?` (`postgresql`, `mysql`, `mariadb`, `redis`, `clickhouse`), `profile?` (a database profile of `db.driver.list`, such as `cockroachdb`: sets the driver and, for a new connection, the port, database, TLS mode and settings not given; a `url` with a profile's scheme, `cockroachdb://`, `redshift://`, `valkey://`, or a managed service's host, `*.neon.tech`, picks it too; `""` takes it off; an unknown one is refused with code `unknownProfile`), `url?`, `host?`, `port?`, `database?`, `user?`, `auth?` (`password`, `none`), `type?` (`dev`), `readOnly?`, `savePassword?` (true), `password?`, `folder?`, `tls?` (`{ mode, caFile, clientCert, clientKey, direct }`), `ssh?`, `navigator?`, `filters?`, `bootstrap?` (`{ defaultSchema, initStatements, isolation }`), `overrides?` (the type's settings for this connection), `settings?` (the driver's: `{"pooler": true}`), `shared?` | the connection (`ssh` is `{ host, user?, port?, jump? }`: a host or `~/.ssh/config` alias, and jump hosts for `ssh -J`); a `password` (or a `url`'s, also `?password=`, removed from the kept URI; `sslpassword` is never kept, in the URI or in `settings`) is stored as a secret in the background, `secretOperation` (kind `storeSecret`); connects nowhere |
| `db.dataSource.update` | `id` and any field of `add` | the connection; absent fields stay; `settings` merge key by key (`null` removes one); an agent may not give `settings.program` or `settings.javaHome` a new value (`agentNotAllowed`, as on `add`), and an agent's change that makes it less safe (read-only off, a looser type or `overrides`) is refused with code `agentNotAllowed` (also for a read-only connection or a type that asks before changes); a new type or new overrides apply to the open sessions at once (a session with uncommitted changes keeps manual commit until they end) |
| `db.dataSource.duplicate` | `id` | the new connection without secrets, like `db.dataSource.add`: the same scope, folder, settings and overrides, named `<name> (copy)` (a number added while taken); its saved secrets are copied in the background (`secretOperation`, kind `storeSecret`) |
| `db.dataSource.remove` | `id` | `{ id, scope, secretOperation }`; its overrides go too, its secrets are deleted in the background (`secretOperation`, kind `deleteSecrets`; a failure is also a notification), a connected one is disconnected (transactions rolled back) |
| `db.dataSource.remove` | `id` | `{ id, scope, secretOperation, closedTabs, unboundConsoles }`; its overrides go too, its secrets are deleted in the background (`secretOperation`, kind `deleteSecrets`; a failure is also a notification), a connected one is disconnected (transactions rolled back); its object, plan, sessions and key tabs and its consoles' result tabs close (`closedTabs`), its open consoles stay without a connection (`unboundConsoles`; `db.console.state` gives `removedConnection`). Unsaved data or structure changes in a tab that would close refuse it (`pendingChanges`, `pendingStructureChanges`, with a `retry`) |
| `db.dataSource.removal` | `id` | `{ id, tabs, consoles, message }`: what removing it would close and unbind, and the confirmation question naming both numbers; changes nothing |
| `db.dataSource.test` | `connection?` and/or any field of `add` (`scope?` for a new one) | `{ operation }`: a session of its own opens, measures a round trip, reads the server's version and closes, in the background; a saved `connection`'s settings are the start and the fields change them for this test only; nothing is saved; without `password` a saved one is used. `db.operation.state` then answers `test { serverVersion, connectMs, roundTripMs, profile? }` (`profile`: the database profile the server turned out to be, by its version text or its host), or the error with `cause` (`dns`, `refused`, `tls`, `authentication`, `timeout`) and a `message` naming what to check |
| `db.driver.list` | `query?` (by id or name), `family?` (`postgresql`, `mysql`, `redis`…), `limit?` (all), `capabilities?` (true) | `{ drivers: [{ id, name, family, defaultPort, capabilities }], profiles: [{ id, name, driver, family, icon, defaultPort, defaultDatabase?, defaultUser?, uriSchemes, hosts, tls, settings, catalogue, hidden, docs, capabilities? }], totalProfiles, truncated }`: the drivers compiled in, in the dialog's order, with their URI schemes, isolation levels and the settings their connections have (`key`, `kind`: `text`, `number`, `bool`, `path`, `choice`; `choices`; `default`); and the database profiles over them (ADR-0173): servers that speak a driver's protocol, with their defaults, the managed services' host names, how their structure is read (`native`, `informationSchema`, `metaFunctions`), the navigator folders they have not, the vendor's docs and their capabilities (the driver's with the profile's schemes and quirks; `capabilities: false` leaves them out) |
| `db.dataSource.test` | `connection?` and/or any field of `add` (`scope?` for a new one) | `{ operation }`: a session of its own opens, measures a round trip, reads the server's version and closes, in the background; a saved `connection`'s settings are the start and the fields change them for this test only; nothing is saved; without `password` a saved one is used. `db.operation.state` then answers `test { serverVersion, connectMs, roundTripMs }`, or the error with `cause` (`dns`, `refused`, `tls`, `authentication`, `timeout`) and a `message` naming what to check |
| `db.driver.list` | none | `{ drivers: [{ id, name, defaultPort, capabilities, external? }] }`: the drivers compiled in, then those plug-ins add as driver programs (`external`: `plugin`, `installed`, `icon`, `process` `perConnection` or `shared`, `protocol`, `running`; ADR-0174, [database-drivers.md](database-drivers.md)), in the dialog's order, with their URI schemes, isolation levels and the settings their connections have (`key`, `kind`: `text`, `number`, `bool`, `path`, `choice`; `choices`; `default`) |
| `db.driver.log` | `id`, `limit?` (50) | `{ lines, truncated }`: an external driver's log, newest lines: its programs started and stopped, what they wrote to standard error and why requests failed, passwords removed, never statements or rows; code `unknownDriver` for a driver that is not external |
| `db.databaseFile.create` | `path` (absolute, or relative to the project's folder) | `{ path }`: an empty SQLite database file, for a new connection's Path (**Create…** in the dialog); a path that exists is refused with code `exists` and never replaced; connects nowhere |
| `db.databaseFile.open` | `path` (absolute, or relative to the project's folder) | `{ path, connection? , offer? }`: what opening a `.db`, `.sqlite`, `.sqlite3`, `.db3` or `.s3db` file from the Project tool window does. When a connection already opens that file, the Database workspace expands and selects it in the navigator (`connection`); otherwise `offer` holds `{ driver: "sqlite", name, database }` for `db.dataSource.add`, which the window shows in the connection dialog. A missing file is refused with code `fileNotFound`; never connects, never creates a file |
| `db.connectionType.list` | none | `{ types, colors }`: Development (`dev`), Test (`test`), Production (`prod`) and the person's own, with `autoCommit`, `confirmExecute`, `confirmDataChange`, `smartCommit`, `autoCloseTransactions`, `endIdleTransactionsAfter`, `closeIdleAfter`, `statementTimeout` (seconds), `readOnly`, `agents` (`full`, `readOnly`, `none`) and `builtIn` |
| `db.connectionType.create` | `id?`, `name`, `color?` (one of `colors`), the settings | the type, starting from Development's settings |
| `db.connectionType.update` | `id`, the settings (0 seconds: never) | the type; connected connections of the type take it at once; built-in types can be changed; an agent's change that loosens `readOnly`, a confirmation or `agents` is refused with code `agentNotAllowed` |
| `db.connectionType.delete` | `id` | the types left; built-in types and types in use are refused with the reason |

The built-in types have these defaults: Development (auto-commit; no confirmations; idle
transactions end after 1800 s; idle connections close after 14400 s), Test (green; confirms data
changes; 900 s, 7200 s), Production (red; manual commit; confirms SQL execution and data changes;
600 s, 3600 s; a statement timeout of 300 s; agents only read). A project connection naming a type the
person does not have is Production. A connection's `overrides` may set `confirmExecute`,
`confirmDataChange`, `autoCommit` and `agents` for it alone.

### Detected databases and imports

The open project's `.env` files (`.env`, `.env.*`) and Compose files (`docker-compose*.yml`,
`compose*.yaml`) are scanned in the background ([ADR-0165](adr/0165-database-connections-types-and-secrets.md),
decision 6): four folders deep, at most 20 000 entries and 64 files of 256 KB, leaving out `.git`,
`.idea`, the project's excluded folders, `node_modules` beside a `package.json` and folders Git
ignores (the `.env` files themselves are read even when Git ignores them). A change to such a file
scans again. They name databases through `DATABASE_URL`, `*_DATABASE_URL`, `REDIS_URL`,
`MONGODB_URI`, `MONGO_URL`, `CLICKHOUSE_URL`, `ELASTICSEARCH_URL`, `OPENSEARCH_URL` (also `jdbc:`
URLs), the `PG*`, `POSTGRES_*` and `MYSQL_*` groups, and Compose services whose image is PostgreSQL
(also PostGIS, TimescaleDB, pgvector), MySQL, MariaDB, Redis, Valkey, MongoDB, ClickHouse,
Elasticsearch, OpenSearch, Cassandra or ScyllaDB, with the port published for the database's port,
the credentials of the image's environment and `${VAR}` taken from the `.env` beside the file.
Nothing connects; a password found is kept in memory until Add stores it in the Keychain.

Imports read the other tool's files and never change them ([ADR-0165](adr/0165-database-connections-types-and-secrets.md), decision 5): DBeaver's
`data-sources.json` (names, folders, drivers, hosts, ports, databases, user names, `dev`/`test`/`prod`
types, read-only, SSH hosts; never `credentials-config.json`) and a JetBrains project's
`.idea/dataSources.xml` with the user names of `dataSources.local.xml` (never the JetBrains password
store). Entries of drivers the IDE does not have are listed as `unsupported` with the reason; one
that is already a connection (the same origin, or the same driver, host, port, database and user)
is `exists` and not added again. A connection made from either keeps its `origin`
(`{ kind: env | compose | dbeaver | jetbrains, file, key, detection?, fingerprint? }`).

| Command | Parameters | Answers |
|---|---|---|
| `db.detect.list` | `limit?` (50), `includeDismissed?`, `refresh?` | `{ detections: [{ id, kind, file, key, driver, product, name, host, port?, database?, user?, published?, hasPassword, supported, dismissed, connection?, redacted? }], total, truncated, scanning, dismissed }`; offers already added, or the same as a connection, are left out; `connection` names the connection added from it when the file changed since; for an agent, an offer from a file hidden from it (or a Compose file reading such a `.env`) has only `id`, `kind`, `file` and `redacted: true` |
| `db.detect.accept` | `id`, `scope?` (`global`), any field of `db.dataSource.add` | `{ connection, replaced }`: the connection made from the offer (its password into the Keychain), or the earlier one changed when the file changed; an agent is refused (`redacted`) an offer from a file hidden from it |
| `db.detect.dismiss` | `id`, `undo?` | `{ id, dismissed }`; kept in `<data folder>/databases/<project key>/detections.json` with the offer's fingerprint, so a change in the file offers it again |
| `db.import.dbeaver` | `path?` (DBeaver's workspace folder, a project folder in it or a `data-sources.json`; `~/Library/DBeaverData/workspace6` when absent), `scope?` (`global`), `keys?`, `preview?`, `limit?` (50) | `{ files, entries: [{ key, name, product, driver?, host?, port?, database?, user?, folder?, type?, ssh?, status, reason?, connection? }], added, total, truncated }`; `status` is `ready` (a preview), `added`, `exists`, `unsupported` or `notChosen` |
| `db.import.jetbrains` | `path?` (a project folder or its `.idea`; the open project when absent), `scope?`, `keys?`, `preview?`, `limit?` | as `db.import.dbeaver` |

The window action `view.dataSource.import { from?: "dbeaver" | "jetbrains", path? }` (Find Action:
**Import Connections from DBeaver** or **from JetBrains Project**; the Overview's **Import
Connections…**) opens the dialog that lists a file's entries with `preview` and imports the ticked
ones; it shows in `get_window_state` as the popup `importConnections`.

**SQLite** connections (driver `sqlite`) keep the database file's path in `database`: absolute,
`:memory:`, or, for a project connection, relative to the project's folder (resolved when it
connects). `readOnly` opens the file read-only; the setting `program` is the path of the `sqlite3`
program when it is not on `PATH`. Connecting never creates a file: a missing one fails with code
`fileNotFound`, a missing program with `sqliteMissing`.

### The connection lifecycle

Every database operation runs on one tokio runtime on the `zen-db` thread, never on the UI thread
([ADR-0164](adr/0164-database-engine-zen-db-runtime-and-drivers.md)). A command **starts an
operation and answers at once** with its id; `db.operation.state` reads where it is. Nothing
connects by itself: only `db.connection.connect` (or the Overview's Connect) opens a connection;
one closed by idle time or sleep opens again when it is next used.

- **Contexts.** Connect opens **Main**, then **Metadata** (the IDE's own SQL: read-only, auto-commit,
  `lock_timeout` 10 s and `idle_in_transaction_session_timeout` 60 s); each console gets its own
  session with its own transaction on first use; an embedded driver shares one session.
- **Guards.** Sessions name themselves `application_name = "Zen <IDE>"`; the type's or connection's
  `statementTimeout` is set at connect (Production: 300 s).
- **Failures.** A session idle for 30 s is pinged before use; TCP keepalive finds dead ones. A lost
  session reopens with backoff (0, 0.25, 0.5, 1, 2, 4 s) and gets back only its tracked state
  (schema, database, isolation, auto-commit, read-only; a Redis console's `SELECT` too). A plain
  read in auto-commit lost while running is run once more (a call of a function other than a few
  read-only session ones, such as `DATABASE()`, is not plain); anything else fails with
  `error.code: "notReplayed"` and is never run again.
- **Cancel.** The local work stops and the server is asked to stop (PostgreSQL's CancelRequest, MySQL's
  and MariaDB's `KILL QUERY` on a second connection); the
  operation ends as `cancelled` within a second, or with `error.code: "cancelUnconfirmed"` (the server
  did not confirm; that session is closed) or `"noServerCancel"`. A statement that changes data and
  finished while the cancel was asked for ends as `done` with its result. Count, Refresh and
  navigator reads are stopped the same way. Commit of a PostgreSQL transaction the server rolls back
  fails with `error.code: "rolledBack"`.
- **Idle.** After the type's (or connection's) time an idle transaction is announced (event
  `transactionIdle`: a 10-second countdown in the window, `db.transaction.postpone` keeps it open)
  and then rolled back, with a notification; idle connections are closed after theirs; when the
  computer slept, every connection closes and reopens on next use.
- **Pooler.** `settings.pooler: true` (behind pgbouncer in transaction mode): unnamed statements
  only, auto-commit always, a `BEGIN` is rolled back and refused, and no session settings are sent.

| Command | Parameters | Answers |
|---|---|---|
| `db.connection.connect` | `connection`, `password?` | `{ operation, status }`; a connect while one runs answers that one; without a stored password and `password` refused with code `passwordNeeded`. `password` is kept in memory until it disconnects, never stored |
| `db.connection.disconnect` | `connection`, `transactions?` (`ask`, `commit`, `rollback`) | `{ operation, status }`; with changes pending in an open transaction and `ask` (the default) refused with code `openTransactions` (below); a transaction of reads only is rolled back |
| `db.connection.disconnectAll` | `transactions?` | `{ operations }`, refused the same way |
| `db.connection.invalidate` | `connection`, `transactions?` | `{ operation, status }`: every session closes and opens again with its tracked state; refused the same way |
| `db.connection.status` | `connection?`, `limit?` (50) | `{ connections: [{ connection, state, closedBecause?, error?, operation?, waitingFor?, connectedSeconds?, autoCommit, pooler, detectedProfile?, contexts: [{ context, open, inTransaction, busy, idleSeconds }] }], total, truncated }`; `state` is `disconnected`, `connecting`, `connected`, `closed` or `failed`; while connecting, `waitingFor` is `keychain`, `tunnel` or `network`, and a saved password the Keychain has not given in 30 s fails the connect with code `secretTimeout` (cancel it with `db.connection.cancel`); `detectedProfile` is the database profile the server turned out to be when the connection names another or none (its settings stay; a notification offers **Use … Profile**, which runs `db.dataSource.update { id, profile }`) |
| `db.connection.cancel` | `operation?`, or `connection` and `context?` (`{"kind": "console", "name": "console_1"}`) | `{ operations }` asked to stop |
| `db.operation.state` | `operation`, `limit?` (50 rows) | `{ id, kind, connection?, context?, status, elapsedMs, error?, message?, cause?, execution?, nodes?, test?, reconnected, rowsTruncated }`; the last 200 operations are kept |

**SSH.** A connection with `ssh.host` connects through `ssh -N -L` from a free loopback port, the
system's OpenSSH with the person's `~/.ssh/config` (ADR-0165); the tunnel opens before the first
session (and for `db.dataSource.test`) and closes with the connection. ssh's questions come through
the IDE's askpass helper: a saved `ssh` secret answers the first password or passphrase prompt, the
rest wait for `db.ssh.answer` (three minutes); an unknown host key is never accepted unless
answered `yes`. A tunnel that does not open fails the operation with `error.code: "ssh"` and what
ssh said in `detail`.

| Command | Parameters | Answers |
|---|---|---|
| `db.ssh.prompts` | `limit?` (50) | `{ prompts: [{ id, connection, name, prompt, secret, question }], total, truncated }`: never an answer |
| `db.ssh.answer` | `prompt`, `value?` (absent refuses), `save?` | `{ answered }`; `save` keeps a password or passphrase in the Keychain as the connection's `ssh` secret |

Errors carry `kind` (`connection`, `authentication`, `tls`, `passwordNeeded`, `timeout`,
`cancelled`, `server`, `notConnected`, `openTransactions`, `invalid`, `unsupported`, `refused`), the
server's `detail` with passwords removed and its `code` (SQLSTATE, or `readOnly` for a statement the
engine refused on a read-only session).

### Transactions

Each context has a **commit mode**: `auto` (every statement commits), `manual` (statements wait for
Commit or Rollback; PostgreSQL begins a transaction before the first one, MySQL and MariaDB run with
`autocommit = 0` and report from the server whether one is open) or `smart` (auto-commit
until the first statement that does not start with `SELECT`, `SHOW`, `USE`, `SET` or `EXPLAIN` — for
languages of one command per line, the first the driver counts as a write —, which switches it to
manual; back to auto when that statement fails, and after Commit or Rollback when the connection
type's `autoCloseTransactions` is on). The default comes from the connection type (Production:
manual; smart commit off everywhere); a mode or isolation set for a console is kept for it across
reconnects. Metadata always commits on its own. While a transaction is open its statements are
logged (`statements`, of which `modifying` may have changed data, with their first lines); a
`BEGIN`/`COMMIT` typed in a console is followed.

**Changes pending** — an open transaction with a statement that may have changed data — make
`db.connection.disconnect`, `.disconnectAll`, `.invalidate`, `db.console.close`,
`db.console.chooseConnection` (leaving a connection), `db.transaction.mode` (leaving manual) and
`workspace.tab.close`/⌘W on a console refuse with code `openTransactions` unless `transactions` is
`commit` or `rollback`. The refusal's data is `{ error, pending: [transaction], retry: { command,
params } }`: run `retry.command` with `retry.params` plus `transactions` to go on; the window asks
Commit / Rollback / Cancel the same way.

| Command | Parameters | Answers |
|---|---|---|
| `db.transaction.commit`, `.rollback` | `path?` (a console; the active one, or with a table's or another tab without a file in front, the main context of that tab's connection), or `connection` and `context?` (`{"kind": "main"}`) | `{ operation, connection, context }`; ⌃4, ⌃8 (also ⌃⌘K, ⌃⌘Z) in the Database workspace; an agent's Commit where agents only read is refused (`agentReadOnly`) |
| `db.transaction.mode` | the target, `mode?` (`auto`, `manual`, `smart`), `isolation?` (the driver's name; `""` the server's default), `transactions?` | the transaction state; an isolation level the connected driver does not name is refused with code `invalid` |
| `db.transaction.status` | the target, `limit?` (50) | `{ connection, name, context, mode, autoCommit, isolation?, inTransaction, statements, modifying, ageSeconds?, idleSeconds?, endingInSeconds?, log: [{ summary, modifying, failed, affected?, secondsAgo }], truncated }` |
| `db.transaction.pending` | `limit?` (50) | `{ transactions, total, truncated }`: every open transaction of the window, without logs |
| `db.transaction.postpone` | the target | `{ postponed }`: an announced idle rollback is called off |
| `db.console.close` | `path?`, `transactions?` | `{ path, operation? }`: the console's session ends, then its tab closes |

`db.console.state` answers the console's `transaction` (with its last 10 statements). The window
action `view.database.pendingTransactions` opens Pending Transactions and
`view.database.transactionMode` the toolbar's mode menu, `view.database.newConsole` (⌃]) a console on
the connection in front as the New Console button does (New Connection… when there is none) and
`view.database.newConnection` (⌃⇧N) the connection dialog. With no console in front the toolbar
keeps the connection, its lock, the mode, Commit, Rollback and the counter of the tab's connection
(else the navigator's selection), on its main context (ADR-0186).

### Safety: read-only, confirmations and agents

([ADR-0172](adr/0172-production-safety-and-agents-on-databases.md).) Every statement is judged
before it runs by our own tokens of the connection's dialect (`zen_db::safety`): what it does
(`read`, `session`, `transaction`, `data`, `structure`, `other`) and its risks (`deleteWithoutWhere`,
`updateWithoutWhere` — also with a `WHERE` that is always true, and inside `WITH` —, `drop`,
`truncate`, `flush`). Comments and strings never count; a read that calls a function with side
effects is a change; anything unknown is `other`.

- **Read-only.** A connection is read-only by its own `readOnly` (the toolbar's lock) or its type's.
  Its sessions are read-only on the server (PostgreSQL, MySQL, MariaDB, SQLite, ClickHouse, Redis),
  and every statement but reads, session settings and transaction control is refused before it is
  sent, with code `readOnly` and the statements (Redis and Valkey judge their own commands by
  `COMMAND INFO` and refuse writing ones with `error.code` `readOnly`); so are saving edits and changing keys. Refusals
  of a read-only connection cannot be confirmed.
- **Confirmations.** Where the type has `confirmExecute` (Production), statements that change
  something ask; on every type a risk asks. The run is refused with code `confirmExecute` and data
  `{ token, target: { connection, name, type, typeName, color, readOnly, confirmExecute, agents },
  statements: [{ index, text, cut, effect, dangers, risks }], asked, total, truncated, dangerous,
  autoCommit, retry: { command, params } }` (at most 100 statements listed; `autoCommit` and the
  message say whether the context commits each change at once or keeps it in the open transaction). Running `retry.command` with
  `retry.params` (which hold `confirmed: token`) runs them; `skip: [index…]` leaves some out
  (recorded as skipped). The token is bound to the connection and the statements: any change asks
  again. The window asks one statement at a time: Yes, Skip, Yes to All, Cancel; a risk runs only
  after it is ticked (**Run Anyway**), Enter never answers, and the window's shortcuts wait while it
  asks (`get_window_state` names it `executeConfirmation`).
- **Agents** (calls over MCP, `zen_core::macros::by_agent`) follow the type's `agents`: `full` runs
  what a person does and confirms with the token; `readOnly` (Production's default) runs reads in a
  read-only session and refuses every change, saving edits, Commit and changing keys with code
  `agentReadOnly` and `handOff: { command: "db.console.new", params: { connection, text } }` for a
  person to review and run; `none` closes the connection to agents entirely
  ([ADR-0179](adr/0179-connections-closed-to-agents-hide-their-metadata.md)): every `db.*` command
  that names it — by `connection`, `connections`, a console's `path`, a `tab`, a result `id`, a
  comparison's sides, or the console, tab or result in front when left out — is refused with code
  `agentNotAllowed`, and lists leave it out (the navigator, Go to Database Object, results,
  consoles, snapshots, the history, the workspace's tabs). Only the commands about the connection
  itself stay open: `db.dataSource.list`, `get`, `add`, `update` and `test` (not `remove`,
  `removal` or `duplicate`, refused like the rest), `db.connectionType.*`, `db.connection.connect`,
  `disconnect`, `status`, `readOnly`, `setSecret`, the drivers, imports and settings. An agent may make a
  connection or a type safer, never less safe (`agentNotAllowed`). Filters and object reads of an
  agent are judged the same way. On a connection whose driver runs on this computer and reads files
  (the language `sql:duckdb`), whatever its type, an agent's statement that reaches past the
  database (a file read or written: `read_csv`, `FROM 'x.parquet'`, `COPY`, `ATTACH`,
  `EXPORT`/`IMPORT DATABASE`; `INSTALL`, `LOAD`, extension and file-access settings, secrets; other
  servers: `*_scan`, `postgres_query`, `query()`) is refused with code `agentOutsideDatabase` and
  the `handOff`; a person's is not.
- **History.** Refused statements are kept with outcome `refused` and their code (`readOnly`,
  `agentReadOnly`, `agentNotAllowed`, `agentOutsideDatabase`, `skipped`); statements run after a confirmation have
  `confirmed: true`; agents' have `agent: true`.
- **Several connections** (`db.script.runOnConnections`). Each connection of the run is checked by
  its own type, all of them before anything runs: one refusal refuses the whole run, and one
  `confirmExecute` refusal lists every connection with its type and what asks there; its token
  covers the connections in their order and each one's statements. The window's dialog shows them
  all (`get_window_state` names it `severalConnections`), Enter never answers, and the window's
  shortcuts wait while it is open. Their statements have the history source `connections`.

| Command | Parameters | Answers |
|---|---|---|
| `db.connection.readOnly` | `connection`, `readOnly?` (the other way round when absent) | `{ connection, readOnly, own, byType, operations }`: the toolbar's lock; kept as the person's own setting of a project connection and applied at once to its open sessions; an agent may turn it on, never off (`agentNotAllowed`); connects nowhere |

### Statements, consoles and results

A console runs on its **connection**: the one chosen for it (`db.console.chooseConnection`, kept in
its tab's data in `workspace.json`), else the connection its folder is named after
(`consoles/<project key>/<connection id>/`), else the only connection there is; a console in the
folder `local` with several connections asks for a choice (code `noConnection`). Each console runs
in a session of its own (`context: { kind: "console", name: "<folder>/<file stem>" }`); a run
connects first when the connection is not connected, with its stored password (code
`passwordNeeded` otherwise). Every run is a **query**: it starts at once and answers its id.

A **script** is a `.sql` file of the project. It runs only on the connection it is **bound** to,
never on a connection guessed for it: an unbound script is refused with code `noBinding` and
`{ path, connections: [{ id, name }], total, truncated }` to choose from. Its binding (connection and
schema) is kept per project in the IDE's data folder,
`<data folder>/databases/scripts/<project key>.json` (`{ "scripts": { "<path in the project>": {
connection, schema? } } }`), never in the file or the repository, and comes back after a restart. A
console is bound through the same commands; its binding stays with its tab. Its statements are split,
completed and checked in the bound connection's dialect, as a console's are.

**Splitting** is our own line lexer per dialect (`zen_db::sql`, ADR-0164 decision 6, ADR-0077's
model: an edit lexes the lines it touched until a line starts in the state it had). It knows quotes
(`'…'` with `''`, PostgreSQL `E'…'` with backslashes, MySQL's backslashes and backticks, SQL Server's
`[…]`), `--` and `/* */` comments (nested in PostgreSQL and SQL Server, `#` in MySQL), PostgreSQL's
`$$` and `$tag$` bodies, psql meta-commands and MySQL `DELIMITER` lines (client commands, never
sent), SQL Server's `GO`; `;` ends a statement except inside brackets or a `BEGIN … END` block
(`BEGIN;`, `BEGIN TRANSACTION`, `BEGIN WORK`, `BEGIN ISOLATION LEVEL …` are transaction commands;
`END IF`, `END LOOP`, `END CASE` close no block). The dialect is the connection driver's
(`postgres`, `mysql`, `sqlServer`, `standard`; `lines` — one statement per line — for Redis-like
languages). **Execute Statement** runs the selection as it is, else the statement under the caret:
the caret right after a delimiter belongs to the statement before it, and on an empty line the line
before counts; a blank line also ends a statement there (setting `blankLines`: `always`, the
default, outside quotes, comments and `BEGIN … END` blocks; `smart`, only where the text before can
end a statement and a statement keyword follows; `never`). **Execute Script** splits on delimiters
only; an error stops it and rolls back the open transaction (setting `stopOnError`).

**Results** ([ADR-0168](adr/0168-result-paging-without-held-locks.md)) are kept in memory as typed
columns within one budget for the window (256 MiB): outside your transaction a reading query is read
through a portal to the row cap (10 000) and its transaction committed, never leaving a lock; inside
your transaction the portal of the last result stays open and pages are read as you scroll. Fetch
Next and Fetch All read on, from the open portal or by reading the statement again past the rows
kept; DML, `RETURNING` and writable CTEs are never paged. `timestamptz` is written in the session's
time zone (`SET TimeZone`), dates and times as the server writes them.

| Command | Parameters | Answers |
|---|---|---|
| `db.script.split` | `text?` (else `path?`, the active console), `dialect?`, `limit?` (50) | `{ dialect, statements: [{ start, end, line, text, cut, client }], total, truncated }`, in characters |
| `db.console.statement` | `path?`, `offset?` (the caret), `blankLines?` | `{ statement?, selection }`: what ⌘⏎ runs |
| `db.console.nextStatement`, `.previousStatement` | `path?` | the statement the caret moved to |
| `db.console.execute` | `path?`, `newTab?`, `confirmed?`, `skip?`, `parameters?`, `variables?` ([Parameters](#bind-parameters-and-script-variables)) | `{ query, connection, path, statements, connecting }`; ⌘⏎; refusals `confirmExecute`, `readOnly`, `agentReadOnly`, `agentNotAllowed` ([Safety](#safety-read-only-confirmations-and-agents)), `parametersNeeded` |
| `db.console.executeInNewTab` | `path?`, `confirmed?`, `skip?`, `parameters?`, `variables?` | as `execute`, keeping the result tabs; ⌘\\ (also ⌃⌘⏎) |
| `db.console.executeScript` | `path?`, `newTab?`, `confirmed?`, `skip?`, `parameters?`, `variables?` | every statement (of the selection when made); ⌥X (also ⌥⌘⏎) |
| `db.console.executeFromCaret` | `path?`, `newTab?`, `confirmed?`, `skip?`, `parameters?`, `variables?` | from the statement under the caret to the end |
| `db.operation.cancel` | `path?` | `{ query?, several }`: the console's query stops, on the server too; ⌘F2; a run on several connections stops too (`several`), the connections still waiting never run |
| `db.console.chooseConnection` | `path?`, `connection` | `{ path, connection, schema }`; ⌃9 (also ⌥⌘9) opens the selector |
| `db.console.chooseSchema` | `path?`, `schema?` | the same; set in the console's session before its next statement; ⌃0 (also ⌥⌘0) |
| `db.console.schemas` | `path?`, `limit?` (200) | `{ connection, schemas?, reading, total, truncated }`, read through Metadata when connected |
| `db.script.bind` | `path?` (absolute or in the project; the active file), `connection?` (`null` unbinds a script), `schema?` (`null`: the default), `transactions?` | `{ path, connection?, schema? }`; the selectors above a `.sql` file and in the workspace's toolbar |
| `db.script.state` | `path?` | `{ path, console, bound, connection?, connectionName?, color?, state, schema?, dialect, inWorkspace }` |
| `db.script.run` | `path?`, `scope?` (`statement`, `script`, `fromCaret`), `offset?` (characters), `newTab?`, `confirmed?`, `skip?`, `parameters?`, `variables?` (as `db.console.execute`) | `{ query, connection, path, statements, connecting, tab }`: the file opens as a tab of the Database workspace with its results below; ⌘⏎ and ⌥⌘⏎ in a `.sql` file of the code editor, and its gutter's run icons; code `noBinding` when unbound |
| `db.script.targets` | `path?`, `limit?` (2000) | `{ targets: [{ line, offset, label }], total, truncated }`: the run icons of the gutter, one per statement |
| `db.script.list` | `connection?`, `limit?` (100) | `{ scripts: [{ path, name, place, console, connection, schema?, exists, open }], total, truncated }`: bound scripts by path, then consoles |
| `db.script.new` | `connection`, `folder?` (in the project; its root), `name?` (`script.sql`, `script_1.sql`, …), `schema?`, `text?` | `{ path, binding, tab }`; never outside the project or in a hidden folder (code `invalidPath`), never over a file (code `exists`) |
| `db.script.runOnConnections` | `path?` (a console or a `.sql` file; the active one), `connections` (ids, in the order they run, at most 20), `scope?` (`script`, the default; `statement`, `fromCaret`), `offset?`, `stopOnError?` (true), `newTab?`, `confirmed?`, `parameters?`, `variables?` (one set for every connection, [Parameters](#bind-parameters-and-script-variables)) | `{ run, path, running, stopOnError, targets: [{ …, inlined }], tab }` at once: the statements run on each connection in turn, split in its dialect, in the console's session there; the window's Execute on Several Connections. Every connection is checked by its own type before anything runs: one that refuses refuses the whole run with its code (`readOnly`, `agentReadOnly`, `agentNotAllowed`) and `targets: [{ target, code, message, handOff? }]`; if any asks, code `confirmExecute` with one `token` for the run and `targets: [{ target, asks, statements, asked, total, dangerous, truncated }]` naming every connection and its type; codes `noTargets`, `tooManyTargets`, `running`, `parametersNeeded` (every connection's placeholders and variables, each once) |
| `db.script.connectionsState` | `path?`, `waitMs?` (≤ 60 000) | the last run on several connections of the console: `{ run, path, running, stopOnError, targets: [{ connection, name, type, typeName, color, readOnly, confirmExecute, agents, status, query?, statements, executed, rows, updated, elapsedMs, error?, results, confirmed }] }`; `status` is `waiting`, `running`, `done`, `failed`, `cancelled` or `skipped` (after a failure with `stopOnError`); code `noRun` |
| `db.console.state` | `path?` | `{ connection, connectionName, state, color, schema, dialect, statement, ran?, hints, panel: { shown, maximized, active }, tabs, last, running }`: `ran` is the characters of the statements the last run executed until the text is edited (the editor flashes them when the run ends); `last` sums up the last query for the header line (statements, executed, updated, rows, executionMs, fetchMs, messages, error, errorLine); `safety` is the connection's `{ connection, name, type, typeName, color, readOnly, confirmExecute, agents }`; `connections` is its last run on several connections, as `db.script.connectionsState` answers it, and a result tab of such a run names its `connection` |
| `db.console.output` | `path?`, `limit?` (200) | `{ messages: [{ query, statement, severity, text, code }], log: [{ query, index, summary, status, executionMs, fetchMs, rows, affected, error }], truncated }` |
| `db.console.toggleResults`, `.maximizeResults` | `path?` | the panel |
| `db.console.showPanelTab` | `path?`, `tab` (`{"kind": "result", "result": 5}`, `{"kind": "output"}`, `{"kind": "log"}`, `{"kind": "variables"}`, `{"kind": "connections"}`) | the panel |
| `db.query.run` | `connection`, `sql`, `schema?`, `waitMs?` (≤ 60 000), `limit?` (50), `session?` (`agent`), `continueOnError?`, `confirmed?` (a `confirmExecute` token, where the type lets agents confirm), `parameters?`, `variables?` (nothing remembered between calls) | the query with each result's first `limit` rows: `{ id, connection, context, status, elapsedMs, statements: [{ index, start, end, summary, status, executionMs, fetchMs, affected?, rows?, result?, error? }], messages, updated, results: [page], message?, messagesTruncated }` |
| `db.query.state` | `query`, `waitMs?`, `limit?` | the same; the last 100 queries are kept |
| `db.query.cancel` | `query` | `{ cancelled }` |
| `db.results.list` | `path?` or `query?`, `limit?` (50) | `{ results: [{ id, query, statement, connection, context, columns, rows, state, stoppedBecause?, more, total?, executionMs, fetchMs, bytes, table?, title?, pinned, console? }], bytes, budget, total, truncated }` |
| `db.result.page` | `id`, `offset` (0), `limit?` (50, ≤ 1000) | the result with `page` (rows of values), `truncated`, `cellsCut` (values cut to 1000 characters) |
| `db.result.fetchNext` | `id`, `rows?` (the page) | `{ …result, operation? }`; code `notRepeatable` when more rows need running again a statement that may change data, which never runs again |
| `db.result.fetchAll` | `id`, `confirmed` | refused with code `confirmFetchAll` without `confirmed: true` |
| `db.result.count` | `id` | `{ …result, operation }`; the count lands in `total` |
| `db.result.refresh` | `id`, `confirmed?`, `skip?` | runs its statement again: in its console (asking as running it does), or in its object tab's Main context |
| `db.result.pin`, `.rename`, `.close` | `id`, `pinned?` / `title` | the tab; closing frees the memory |
| `db.result.view` | `view` (`grid`, `text`, `record`, `json`: documents of a document database; others show as a grid) | remembered for every result |
| `db.result.copy` | `id?`, `rows?` (`[first, last]`), `columns?`, `header?`, `advanced?`, `settings?` (`header`, `rowNumbers`, `columnDelimiter`, `rowDelimiter`, `quote`, `quoting`: `needed`, `always`, `never`; `nullText`, `values`: `display`, `sql`, `json`), `remember?`, `clipboard?`, `limit?` (100 000 characters) | `{ text?, rows, totalRows, chars, truncated, cut? (rows, size, limit, cancelled), message?, job? }`: tab-separated, quoted where needed (≤ 100 000 rows, 64 MiB); with `clipboard` the text goes to the clipboard, over 50 000 cells in the background (`job`); codes `emptyDelimiter`, `delimiterQuote` |
| `db.result.copyAs` | as `db.result.copy` without the options, `format` (`csv`, `tsv`, `json`, `markdown`, `html`, `xml`, `sqlInsert`, `sqlUpdate`, `sqlIn`, `go`, `rust`, `typeScript`, `python`, `kotlin`, `columnNames`), `settings?` (as `db.export.start`'s) | the same; `sqlUpdate` needs the rows' table (code `readOnly`) |
| `db.result.copyState` | `job?`, `cancel?` | `{ jobs: [{ job, result, status (running, done, cancelled), rows, totalRows, cut?, message? }] }`, never the text; code `unknownJob` |
| `db.copy.settings` | `settings?`, `format?`, `pasteNullText?` | `{ version, settings, format, pasteNullText, formats: [{ id, name }] }`, kept in `<data folder>/databases/copy.json` |
| `db.settings.get`, `db.settings.update` | `blankLines?`, `page?`, `rowCap?`, `keepOpen?`, `budgetMb?`, `stopOnError?`, `view?`, `valuePanel?`, `valueFormat?` | the settings, kept in `<data folder>/databases/settings.json` |

Values are the one value model of every database: `{"kind": "integer", "value": 1}`, `"null"`,
`"bool"`, `"decimal"` (text), `"float"`, `"text"`, `"bytes"` (hex), `"temporal"` and `"other"`
(`{ text, type }`), `"interval"`, `"uuid"`, `"json"`, `"array"`, `"map"`, `"document"`. A failed
statement's error has the server's `code` and `position` (one-based, in the statement); the console
underlines it as a problem (source `database`) and marks the statement "failed", and every
statement that ran gets its time and rows after it in the editor. Nothing here connects by itself.

### SQL assistance

Completion, navigation and the warnings of a console come from its connection's snapshot, offline
and on the window's thread in well under a millisecond per statement; nothing connects. The
statement under the caret is split as Execute Statement splits it (blank lines as `smart`) and read
by our own error-tolerant resolver over the dialect's tokens (`zen_db::assist`): its query blocks
(the statement, subqueries, `WITH` bodies) with the tables of their `FROM`, `JOIN`, `UPDATE`,
`INSERT INTO`, `DELETE … USING` lists, aliases, CTE column lists and the output columns of
subqueries (`*` expanded); correlated subqueries see the tables around them. Unqualified names are
looked up in the console's schema (else `public`, `main`, the connection's database or the only
user schema), then `pg_catalog`, then the other schemas of the database. `editor.completion.start`,
`navigate.declaration.goTo` (⌘B, ⌘-click), `editor.documentation.show` (F1) and
`editor.declarationPeek.show` (⌘-hover) answer from it in a console and in a bound script; `problems.file.list` lists the
unknown tables and columns as warnings (source `database`, codes `unknownTable`, `unknownColumn`).

| Command | Parameters | Answer |
|---|---|---|
| `db.sql.complete` | `path?`, `offset?` (characters, else the caret), `limit?` (50) | `{ connection, from, prefix, items: [{ label, kind, insert, detail? }], total, truncated, loading }`; `kind` is `keyword`, `schema`, `table`, `view`, `column`, `function`, `alias`, `join`, `command` or `key`; `loading` while columns missing from the snapshot are read |
| `db.sql.resolve` | `path?`, `offset?`, `limit?` (50) | `{ connection, range, target: { kind: "schema" \| "object" \| "column", path, column? }, description? }` with the object's kind, comment, primary key and columns (at most `limit`, `truncated`), or `null` |
| `db.sql.navigate` | `path?`, `offset?` | `{ target, tab }`: opens the object (a column on its table's Columns), as F4 does; code `unknownName` when nothing is known there |
| `db.sql.format` | `text` (at most 1 MiB), `dialect?` (`postgres`, `mysql`, `sqlServer`, `standard`), `connection?` (its driver's dialect; nothing connects), `options?` (`{ keywordCase: "asIs" \| "upper" \| "lower", commas: "trailing" \| "leading", clauseOnOwnLine, itemPerLine }`, else Code Style → SQL), `indent?`, `useTabs?`, `limit?` (20 000 characters, at most 200 000) | `{ text, truncated, changed, statements, kept }`: the text formatted as Reformat Code formats a console; `kept` counts statements left as they were (an unclosed quote or comment, unmatched brackets) |

**SQL in code** (ADR-0183): a string literal of a Go, Rust, Python, JavaScript, TypeScript, Java or
Kotlin file holds SQL when a `language=SQL` comment (or another SQL dialect's name) is on its line or
the line before, when a `sql` tag stands before a template literal, or when its text reads as a
statement (`SELECT …` in capitals, `select … from`, `INSERT INTO`, `UPDATE … SET`, `DELETE FROM`,
`WITH … AS`, `CREATE TABLE` and other DDL). Its escapes are read (`\"`, `\n`), not in raw literals.
Such a fragment is highlighted as SQL in every IDE; with the database module, `editor.completion.start`,
`navigate.declaration.goTo`, `editor.documentation.show`, `editor.declarationPeek.show`,
`db.sql.complete`, `db.sql.resolve` and `db.sql.navigate` work at a place inside it, and
`problems.file.list` lists its unknown tables and columns, on the snapshot of the file's connection:
its own (`db.injection.bind`), else the project's default (`db.injection.setDefault`). Without one,
completion offers keywords and the `db.sql.*` commands refuse with code `noBinding` (with
`connections`, `total`, `truncated`); its message names `db.injection.bind` and
`db.injection.setDefault` only to an agent (`database::for_caller`). Nothing connects. In a literal
with SQL, `editor.intentions.list` adds Run SQL
(id `db.injection.run`) and the choice of the file's connection (id `db.injection.choose`, which the
window answers with a chooser at the caret; `editor.intentions.apply` refuses it with code
`chooseConnection` and the connections, as agents bind with `db.injection.bind`; a person's message
names Choose SQL Connection… instead).

| Command | Parameters | Answer |
|---|---|---|
| `db.injection.list` | `path?` (absolute or in the project; the active file), `limit?` (50) | `{ path, connection?, schema?, binding: "file" \| "project" \| "none", fragments: [{ start, end, line, language, text, textTruncated }], total, truncated }`; `start`/`end` are characters of the SQL inside the quotes, `text` its first 200 characters with escapes read; code `notCode` for other files |
| `db.injection.bind` | `path?`, `connection?` (absent: follow the project's default), `schema?` | `{ path, connection?, schema?, binding }`: the binding in effect, kept with the scripts' bindings in the data folder |
| `db.injection.setDefault` | `connection?` (absent: clear), `schema?` | `{ connection?, schema? }` |
| `db.injection.run` | `path?`, `scope?` (`statement`: the selection inside the literal, else the statement under the caret; `script`: every statement of the literal; `fromCaret`), `offset?` (characters of the file), `newTab?`, `confirmed?`, `skip?`, `parameters?`, `variables?` (as `db.console.execute`) | `{ query, connection, path, statements, connecting, tab }`: runs the SQL of the literal at the caret on the file's connection (its own, else the project's default) in the file's own session; the file opens as a tab of the Database workspace with its results below, and `db.console.state`, `db.console.statement`, `db.console.execute`, `db.console.explain` take the code file as they take a console. Codes `notCode`, `noFragment` (no literal with SQL there), `noBinding` (with `connections`), `parametersNeeded` (placeholders `$1`, `?`, `:name`; the retry names `db.injection.run`), `confirmExecute` and the other safety refusals of the connection's type (ADR-0172). ⌘⏎ in a code file, the run icon of the literal's line and the ⌥⏎ action Run SQL (`editor.intentions.list` id `db.injection.run`) run it |

**Find Usages of tables and columns.** `db.usages.find` lists where a table, view, routine or column
is used: the project's `.sql` files and SQL strings in code bound to the connection (or to none),
its consoles, and the snapshot's views, routines, triggers and foreign keys, names resolved as ⌘B
resolves them, on a tab of the Find tool window that `search.results.tree` reads (kind `database`;
a usage in the database has `command` instead of `path`). It runs in the background; `waitMs` waits
for the end. `navigate.usages.find` at a database name in SQL does the same. `db.usages.rename`
opens the usages as a rename to `newName`: `search.results.replace` replaces them in project files
only, never in consoles or the database. Neither connects; without the connection's structure they
refuse with `noStructure`, schemas, sequences and types with `unsupported`, an unknown name with
`unknownObject`.

| Command | Parameters | Answer |
|---|---|---|
| `db.usages.find` | `connection?` and `path?` (`[db, schema, folder, object]`, or a column's `[db, schema, folder, table, "columns", column]`), `column?`; or `file?` and `offset?` (a name in an open console, SQL file or code file); else the active object tab; `newTab?`, `waitMs?` (at most 10 000), `limit?` (50) | `{ tab, connection, target, label, done, usages: [{ path?, holder?, line, column, text, access? }], total, truncated }` |
| `db.usages.rename` | as `db.usages.find`, and `newName` | as `db.usages.find`, with `newName` |

Reformat Code (`editor.code.reformat { path?, waitMs?, wholeFile? }`) formats consoles and SQL files
itself, in the background, with the options of the setting `sqlFormat` (Code Style → SQL) and the
dialect of the console's or file's connection (else the file's extension): with a selection in the
active file only the statements it touches, unless `wholeFile`; `pending` while it runs, and running
it again waits on. Only white space and the case of reserved keywords change; a statement whose
tokens would differ is kept as it was.

Any `.sql` file, bound or not, also gets the SQL plug-in's folding, Structure, keyword completion
and syntax problems ([ADR-0182](adr/0182-sql-plugin-abi-v1-and-dialects.md)) through the editor's
commands: `editor.fold.*`, `editor.structure.get`, `editor.completion.start` and
`problems.file.list` (code `syntax`). In a console or a bound file the plug-in reads the text in the
connection's dialect, and `editor.completion.start` answers from the connection only; nothing
connects.

### Export

Rows are exported in the background, streamed from the server to files or the clipboard, never
through the results kept in memory
([database-workspace.md](database-workspace.md#export)). Answers never hold rows or
values.

| Command | Parameters | Answers |
|---|---|---|
| `db.export.settings` | `source?` (as `db.export.start`'s) | `{ version, settings, folder?, pattern, clipboard, defaultFolder, formats: [{ id, name, extension }], source?: { name, connection, connectionName, serverCopy } }`, kept in `<data folder>/databases/export.json`; `serverCopy` says whether the server writes CSV and TSV itself (PostgreSQL's `COPY`); never connects |
| `db.export.start` | `source` (`{ kind: result, id? }`, `{ kind: console, path?, sql? }`, `{ kind: query, connection, sql, name? }`, `{ kind: tables, connection, paths }`, `{ kind: cells, id?, rows?, columns? }`: cells of a result as loaded, `rows` and `columns` as `[first, last]`, written from memory in the background, never Parquet), `settings?` (`format`: `csv`, `tsv`, `json`, `sql`, `markdown`, `html`, `xml`, `parquet` (files only); `delimiter`, `quote`, `quoteAll`, `header`, `nullText`, `lineEnding` (`lf`, `crLf`), `escapeFormulas`, `bom`, `json` (`array`, `lines`), `table`, `batch`, `serverCopy`, `compression` (`snappy`, `gzip`, `lz4`, `none`) and `rowGroup` (1 000–1 000 000) for Parquet), `clipboard?`, `folder?`, `pattern?`, `path?`, `overwrite?`, `remember?` | `{ id, job?, items: [{ name, path? }], clipboard, connecting }` (`id` 0 and the copy `job` for `cells`, followed with `db.result.copyState`); codes `exists` (with `paths`), `noFolder`, `notATable`, `noStatement`, `severalItems`, `delimiterQuote`, `emptyDelimiter`, `binary` (Parquet to the clipboard, or cells as Parquet), `exportChanges` (a statement that may change data, never run) |
| `db.export.state` | `id?`, `waitMs?` (≤ 60 000), `limit?` (20) | `{ exports: [{ id, format, status, elapsedMs, current?, rows, bytes, items: [{ name, connection, path?, status, rows, bytes, serverCopy, error? }], text }], truncated }` |
| `db.export.cancel` | `id` | `{ cancelled }`: the query stops on the server and the file being written is removed |

The window action `view.database.export` opens the Export Data dialog (`{ source? }` or
`{ connection, path }` of a table, schema or folder); `get_window_state` lists it as `exportData`.
`run_action` takes every window action that opens the dialog of a `db.*` command
(`view.database.export`, `view.database.import`, ...), with or without parameters.

### Backup and restore

Backups and restores run the database's own tools as runs of the Run tool window
([database-workspace.md](database-workspace.md#backup-and-restore)). Answers never hold
passwords: the tool gets one only in its environment.

| Command | Parameters | Answers |
|---|---|---|
| `db.backup.settings` | none | `{ version, folder?, pgDump, mysqldump, pgRestore, mysql, defaultFolder }`, kept in `<data folder>/databases/backup.json` |
| `db.backup.tools` | `connection`, `refresh?`, `waitMs?` (≤ 60 000) | `{ connection, family, status: searching/done, tools: [{ tool, origin, program, container?, docker?, version?, major? }], docker: { state: notLocal/notFound/notRunning/noContainer/container, container? }, chosen }`; `tool` is `pgDump`, `pgRestore`, `psql`, `mysqldump`, `mysql`; `origin` is `path`, `homebrew`, `app`, `docker`; codes `unsupported`, `sshTunnel` |
| `db.backup.start` | `connection`, `database?`, `file?` or `folder?`, `tool?` (`{ origin?, program? }`), `pgDump?` (`format`: `plain`, `custom`, `directory`, `tar`; `compression`, `content`: `all`, `schemaOnly`, `dataOnly`; `schemas`, `tables`, `excludeTables`, `inserts`, `clean`, `ifExists`, `create`, `noOwner`, `noPrivileges`, `encoding`, `jobs`, `verbose`, `extraArgs`), `mysqldump?` (`content`, `tables`, `method`: `singleTransaction`, `lockTables`, `none`; `addDropTable`, `routines`, `events`, `triggers`, `extendedInsert`, `completeInsert`, `hexBlob`, `comments`, `verbose`, `extraArgs`), `overwrite?`, `password?`, `remember?` | `{ runId, kind, database, file, tool? }`; codes `exists` (with `paths`), `noFolder`, `relativePath`, `noDatabase`, `passwordNeeded`, `unsupported`, `sshTunnel`, `agentNotAllowed` |
| `db.backup.restore` | `connection`, `file`, `database?`, `tool?`, `pgRestore?` (`content`, `clean`, `ifExists`, `create`, `noOwner`, `noPrivileges`, `singleTransaction`, `exitOnError`, `jobs`, `verbose`, `extraArgs`), `mysql?` (`force`, `extraArgs`), `confirmed?`, `password?`, `remember?` | `{ runId, kind, database, file }`; always refused first with `confirmExecute` (`token`, `target`, `retry`); codes `noFile`, `readOnly`, `agentReadOnly`, `agentNotAllowed`, `passwordNeeded` |
| `db.backup.state` | `runId?`, `waitMs?` (≤ 60 000), `limit?` (20) | `{ jobs: [{ runId, kind, connection, database, file, tool?, status: running/finished/failed/stopped, exitCode?, error?, elapsedMs, bytes? }], truncated }` |
| `db.backup.cancel` | `runId` | `{ cancelled }`: the tool is stopped and a backup's unfinished file removed (also `run.process.stop`) |

The tool's console is read with `run.output.get`. `run.process.rerun` on a backup run starts it again
with the same tool and options and replaces its file; on a restore run it answers the
`confirmExecute` refusal of `db.backup.restore` (and `passwordNeeded` when the password was typed for
one run), and the window's dialog asks. The window actions `view.database.backup` and
`view.database.restore` open the dialogs (`{ connection?, path?, database?, file? }`; the
navigator's selection when absent); `get_window_state` lists them as `backupDatabase`.

### Import from CSV

A CSV file is imported into a table in the background, read record by record and never whole
([database-workspace.md](database-workspace.md#import-from-csv)). Answers never hold
rows or values, apart from the preview's first rows, which the caller asked for.

| Command | Parameters | Answers |
|---|---|---|
| `db.csvImport.settings` | none | `{ version, format, load, folder? }`, kept in `<data folder>/databases/import.json` |
| `db.csvImport.preview` | `file`, `format?` (`delimiter`, `quote`, `escape?`, `header`, `nullText`, `timestampFormat?`), `limit?` (20, ≤ 100), `connection?`, `table?` (`[db, schema, "tables", name]`), `create?` | `{ columns: [{ name, kind }], rows, truncated, fileBytes, format, target?: { path, qualified, exists, columns: [{ name, type, kind }], loading }, mapping: [{ source, target, kind, sqlType? }] }`; `kind` is `text`, `integer`, `decimal`, `boolean`, `date` or `timestamp`; codes `noFile`, `io`, `delimiterQuote`, `lineBreak`, `notATable` |
| `db.csvImport.start` | `connection`, `file`, `table`, `create?`, `format?`, `columns?` (`[{ source, target, kind?, sqlType? }]`), `load?` (`batch` 1000, `commitEvery` 0, `truncate`, `onError` `stop`/`skip`, `serverCopy`), `confirmed?`, `remember?` | `{ id, table, statements, columns, connecting }`; codes `exists`, `noColumns`, `badMapping`, `readOnly`, `confirmExecute` (Production, `truncate`; with `token` and `retry`), `agentReadOnly`, `agentNotAllowed` |
| `db.csvImport.state` | `id?`, `waitMs?` (≤ 60 000), `limit?` (20) | `{ imports: [{ id, connection, file, table, status, elapsedMs, fileBytes, bytesRead, rowsRead, rowsInserted, rowsCommitted, rowsSkipped, serverCopy, errors: [{ row, line, column?, code, detail }], errorsTotal, error? }], truncated }`; `code` is `columns`, `notANumber`, `notABoolean`, `notADate` or the server's error code |
| `db.csvImport.cancel` | `id` | `{ cancelled }`: the running statement stops on the server and what is not committed is rolled back |

The window action `view.database.import` opens the Import from CSV dialog (`{ connection, path }` of a
table, a schema or its Tables folder; the navigator's selection when absent); `get_window_state`
lists it as `importData`.

### Data transfer

Tables of one connection are copied into another (or another schema of the same) in the background,
read page by page and written in batches, never held whole
([database-workspace.md](database-workspace.md#data-transfer)). Answers never hold
rows or values.

| Command | Parameters | Answers |
|---|---|---|
| `db.transfer.preview` | `source`, `tables` (`[[db, schema, "tables", name]]`), `target`, `schema?` (`[db, schema]`; the one named as the source's, else the first), `limit?` (20 schemas) | `{ sourceDriver, targetDriver, schema?, schemas, truncated, tables: [{ source, target, qualified, exists, create, columns: [{ source, sourceType, target, type, nullable }], key, rows?, loading }], load }`; never connects; codes `unsupported`, `notATable`, `notFound`, `badSchema` |
| `db.transfer.start` | `source`, `target`, `schema`, `tables` (`[{ source, name?, create?, columns?: [{ source, target?, type? }] }]`), `load?` (`batch` 1000, `commitEvery` 0, `truncate`, `onError` `stop`/`skip`, `serverCopy`), `confirmed?`, `remember?` | `{ id, tables: [{ name, table, statements, columns }], connecting }`; codes `noTables`, `exists`, `noColumns`, `badMapping`, `loading`, `readOnly`, `sameSession`, `confirmExecute` (with `token` and `retry`), `agentReadOnly`, `agentNotAllowed` |
| `db.transfer.state` | `id?`, `waitMs?` (≤ 60 000), `limit?` (20 transfers, 20 row errors a table) | `{ transfers: [{ id, source, target, status, elapsedMs, current?, rowsRead, rowsInserted, rowsCommitted, rowsSkipped, tables: [{ name, table, status, rowsRead, rowsInserted, rowsCommitted, rowsSkipped, serverCopy, errors: [{ row, line, column?, code, detail }], errorsTotal, error? }] }], truncated }` |
| `db.transfer.cancel` | `id` | `{ cancelled }`: reading and writing stop and what the current table has not committed is rolled back |

The window action `view.database.transfer` opens the Transfer Data dialog (`{ connection, path }` of a
table, a schema or its Tables folder; the navigator's selection when absent); `get_window_state`
lists it as `transferData`.

### Find in Database

Find in Database looks for a text in the rows (and, with `metadata`, the names) of one schema's
tables on the server ([database-workspace.md](database-workspace.md#find-in-database)).
The catalogue and every table are read with SQL generated in the driver's dialect (ADR-0170), in a
read-only, auto-commit session of the search's own, one table at a time with a row limit and a
deadline each, so no lock outlives a table's read (ADR-0168). Only counts come back: answers never
hold values. Every statement goes into the query history (source `search`); agents follow their
connection type's rules (`agentNotAllowed` where agents may run nothing).

| Command | Parameters | Answers |
|---|---|---|
| `db.search.targets` | `connection`, `schema?`, `views?` | `{ connection, sql, connected, schemas, truncated, schema?, estimatedRows?, warnings: [{ code, message }] }` from the kept structure, never connecting; warning codes `production`, `largeSchema` (over a million rows by the server's estimates), `views` |
| `db.search.open` | `connection?`, `schema?`, `text?` | the Find in Database tab, brought to front with its form filled in (the text is never written to disk) |
| `db.search.start` | `connection`, `text`, `schema?` (the default one), `data?` (true), `metadata?`, `caseSensitive?`, `columnKinds?` (`text` (default), `number`, `temporal`, `other`), `columns?`, `tables?` (names, any case), `views?`, `limitPerTable?` (100, ≤ 10 000), `tableTimeoutMs?` (10 000, ≤ 600 000) | `{ id, connection, schema, connecting, cancelled?, warnings }` at once; the window's running search is cancelled; codes `noText`, `nothingToSearch`, `notSql`, `noSchema`, `agentNotAllowed` |
| `db.search.state` | `id?` (the window's last), `waitMs?` (≤ 60 000), `limit?` (50, ≤ 5 000), `all?` | `{ search?: { id, connection, schema, text, caseSensitive, data, metadata, limit, tableTimeoutMs, status, elapsedMs, listed, current?, ended, rows, found, catalogTruncated, tables: [{ table, kind, status, nameMatches?, namedColumns?, searchedColumns, rows, truncated, columns: [{ name, rows }], elapsedMs, error? }], error? }, tables, truncated }`; tables with the text, stopped (`timeout`) or failed unless `all` |
| `db.search.cancel` | `id?` | `{ cancelled }`: the table being read stops on the server |
| `db.search.openResult` | `id?`, `table`, `columns?` | `{ tab?, condition?, query?, path?, run? }`: the table's object tab on Data, filtered to the rows with the text (the filter is in its history; Back returns); a table the navigator does not know, or a driver with its own tree, opens a console running the filtered `SELECT`; codes `unknownTable`, `noMatchingColumns` |

### Structure and the Database Navigator

A connection's structure is kept as a **snapshot** in the IDE's caches
(`<data folder>/caches/databases/<global or project key>/<id>.snapshot`), read before connecting
and refreshed in the background on connect, after DDL and on Refresh; per-schema fingerprints over
the system catalogues decide what is read again ([database-workspace.md](database-workspace.md),
"Structure and its snapshot"). A **path** names a node of a connection's tree: `[]` its databases,
`[db]` schemas, then on PostgreSQL the folders `#extensions`, `#eventTriggers`, `#tablespaces` and
`#roles` with their items (`[db, #roles, name]`), `[db, schema]` folders (`tables`, `foreignTables`,
`views`, `materializedViews`, `functions`, `procedures`, `sequences`, `types`; on PostgreSQL also
`indexes`, every index of the schema as `[db, schema, indexes, name]`, and `aggregates`),
`[db, schema, folder, object]` an object (a routine as `name(arguments)`), then `columns`,
`constraints`, `foreignKeys`, `indexes`, `triggers`, `references`, `policies`, `partitions` and their
items. A driver's own tree has its own paths: Redis
`["db0"]` (in a cluster `["host:port"]`), `["db0", "user:"]` for the keys `user:…` (a prefix keeps
its separator), `["db0", "user:", "1"]` for the key `user:1`; ClickHouse `[database]`,
`[database, tables | views | dictionaries]`, `[database, folder, table]`, then a column. Answers hold structure only, never data values or secrets; none
of these commands connects, except `db.navigator.expand` on a disconnected connection, which is the
person's action.

| Command | Parameters | Answers |
|---|---|---|
| `db.schema.children` | `connection`, `path`, `limit?` (200) | `{ nodes: [{ path, kind, label, hasChildren, detail?, system?, size?, rows?, comment? }], total, truncated, loading, operation?, message? }`: from the snapshot at once; a node not read yet on a connected connection starts reading (`loading`, `operation`) |
| `db.schema.refresh` | `connection`, `path?`, `force?` | `{ operation?, started }`: reads again what the fingerprints say changed (`force`: all), or one schema or object by `path`; a disconnected connection answers `started: false` |
| `db.object.get` | `connection`, `path`, `limit?` (200) | `{ connection, path, catalog, schema, qualified, driver, object, loading, operation?, truncated }` (`qualified` quoted for the connection's SQL: backticks for MySQL and MariaDB, whose databases are the schemas of the catalogue `def`; `db.navigator.*` rows show them right under the connection, their keys keeping the full path): kind, owner, comment, size, row estimate, columns (type, nullable, default, identity, generated, collation, comment), constraints, foreign keys with their references, indexes, triggers, policies, partitioning, a view's query, a routine's signature and source, a sequence's or type's settings; lists at most `limit`, definitions cut at 8000 characters |
| `db.object.ddl` | `connection`, `path` (a schema's too), `nested?`, `permissions?`, `separateForeignKeys?`, `separateConstraintIndexes?`, `comments?`, `partitions?` (all false) | `{ ddl?, loading, operation? }`: generated offline from the snapshot; an index, constraint or trigger by its item path |
| `db.object.open` | `connection`, `path` (`[db, schema, folder, object]`, or `[db, schema]` for a schema), `page?` | `{ tab, kind, page, query? }`: the object's tab in the Database workspace (see below), on Data for tables and views, on Properties for the rest; reads the rows only when the connection is connected. In a driver's own tree (Redis, ClickHouse) a key opens its value tab (`db.key.open`) and a table or view a new console with the driver's `SELECT`, answering `{ tab, kind, page, path, text }` |
| `db.object.copyName` | `connection`, `path`, `qualified?` | `{ name }`: the name, or qualified by schema (and table) and quoted as the connection's SQL needs it; a key-value database's whole key or prefix |
| `db.key.get` | `connection`, `path`, `limit?` (100, ≤ 1000), `waitMs?` (0, ≤ 60 000) | `{ operation, status, key?, cellsCut, message?, error? }`: reads a key in the background, connecting first with the stored password; `key` is `{ key, database, type (string, hash, list, set, zset, stream, json, none), ttlMs?, size?, length?, encoding?, columns, rows, truncated }`, values cut to 1000 characters; `unsupported` for a connection that is not key-value |
| `db.key.expire` | `connection`, `path`, `seconds?` (absent: never expire), `waitMs?` | the same answer after `EXPIRE` or `PERSIST`; a read-only connection fails it with `error.code` `readOnly`; an agent where agents only read is refused (`agentReadOnly`) |
| `db.key.state` | `operation`, `waitMs?` | the same answer for an operation of `db.key.get` or `db.key.expire` |
| `db.key.open` | `connection`, `path` | the tab: kind `key`, titled with the key, data `{ connection, path, database, key }`; the workspace expands and the tab reads the key |
| `db.navigator.state` | `offset?`, `limit?` (200) | `{ rows: [{ key: { connection?, folder?, path? }, depth, kind, label, detail?, comment?, hasChildren, expanded, error?, selected, system?, size?, sizeRatio?, rows?, state?, readOnly?, color?, driver? }], total, truncated, filter, options, selected? }`: the visible rows in order; `kind` is `connectionFolder`, `connection`, `loading`, `message` or a node's kind |
| `db.navigator.expand` | `connection?`, `folder?`, `path?` (the selected row when none) | `{ key, connecting? }`: a disconnected connection connects (`passwordNeeded` without a saved password) |
| `db.navigator.collapse` | `connection?`, `folder?`, `path?`, `all?` | `{ collapsed }`: the row and its descendants, or every row |
| `db.navigator.select` | `connection?`, `folder?`, `path?` | the row's key; its ancestors expand |
| `db.navigator.refresh` | `connection?`, `folder?`, `path?` (the selected row when none) | `{ operation?, started }`: F5 and ⌘R in the navigator; expanded rows stay expanded |
| `db.navigator.filter` | `text?`, `connectedOnly?` | the first 200 rows: rows whose names contain `text` among what was read, with their ancestors |
| `db.navigator.create` | `connection`, `path` (the navigator row), `kind` (`table`, `view`, `sequence`, `function`, `schema`; `column`, `index` under a table) | `{ tab, kind }`: Create ▸; opens the schema's or table's tab on Properties and the window's form for the new object; nothing runs until it is staged (`db.structure.stage` on `tab`) and executed; `notCreatable` without such a parent |
| `db.object.drop` | `connection`, `path` (a schema, an object, or a column, index, constraint or foreign key of a table), `cascade?`, `confirmed?` | `{ query, statements }`: Delete (⌦); runs the `DROP` at once in the Main context after it is confirmed: the first call is refused with `confirmExecute` whose `loss` names what is lost and `cascade` whether the server can cascade, with the statements and `token`; `notDroppable`, `readOnly`, `unsupported` as named; tabs of what was dropped close when it ends |
| `db.object.rename` | `connection`, `path` (a schema, an object or a column), `to`, `confirmed?` | `{ query, statements }`: Rename (F2); runs at once, asking (`confirmExecute`) only where the connection type confirms changes; `invalidName`, `notRenamable`; the object's tabs open again under the new name |
| `db.object.truncate` | `connection`, `path` (`[db, schema, tables, table]`), `cascade?`, `confirmed?` | `{ query, statements }`: Tools ▸ Truncate; `TRUNCATE TABLE` (`DELETE FROM` on SQLite) at once after it is confirmed, as `db.object.drop`; `notTruncatable` for anything but a table |
| `db.navigator.options` | `view?` (`simple`, `advanced`, `custom`), `showSystemObjects?`, `showHost?`, `showDescriptions?`, `showStatistics?`, `linkWithEditor?` | the options in effect, kept per project with the expansion and selection in `<data folder>/databases/<project key>/navigator.json` |

### Object editors

A database object opens as a tab of the Database workspace (`db.object.open`; ↩, F4 and a double
click in the navigator open it on Properties; F4 or Open Object on a schema), with the id
`object:<connection>:<path joined by />` (`/` and `%` in names escaped as `%2F`, `%25`) and the kind
`object`. Its pages are **Properties**, **Data**, **DDL** and **Diagram** for tables, views,
materialised views and foreign tables (`db.object.open` without `page` opens them on Data), and **Properties** and **DDL** for
functions, procedures, sequences, types and schemas. The tab keeps its connection, path, page,
property section and DDL toggles in `workspace.json`, never rows, and comes back after a restart
without connecting. The Data page reads `SELECT * FROM <schema>.<name>`, ordered by the primary key
(else a unique key of non-null columns), in the connection's **Main** context into the result store,
paged as consoles' results are; it reads by itself only when the connection is connected, and
`db.object.readData` (Read Data on the page) connects first. An empty table reads as its columns
and no rows; `data.error` says "Cancelled." only when the read was cancelled, and that the
statement returned no columns when it finished without any. Closing the tab frees its rows. The
DDL page is `db.object.ddl` with the tab's toggles; Open in Console is `db.console.new { connection,
text }`.

| Command | Parameters | Answers |
|---|---|---|
| `db.object.state` | `tab?` (the active object tab) | `{ tab, connection, connectionName, color?, state, path, kind?, name, qualified?, page, pages, section?, ddl, data?: { statement, query?, result?, reading, error? } }`; never data values |
| `db.object.showPage` | `tab?`, `page` (`data`, `properties`, `ddl`, `diagram`) | the state; a page the kind lacks is refused with code `invalidPage`; showing Data reads the rows when connected |
| `db.object.showSection` | `tab?`, `section` | the state on Properties; a section the kind lacks is refused with code `invalidSection` |
| `db.object.properties` | `tab?` or `connection` and `path`, `section?`, `limit?` (200) | `{ connection, path, kind, header: [{ label, value }], sections: [{ id, title, count? }], section, columns, rows, text?, note?, total, truncated, loading }`: the header (name, comment, owner, tablespace where the driver has them; a routine's arguments, result and language), the sections the driver's databases have, each table section with its `count`, and one section as rows of text (Columns without a position column) or as `text` (a view's query, a routine's source); from the snapshot, offline |
| `db.object.ddlOptions` | `tab?`, `nested?`, `permissions?`, `separateForeignKeys?`, `separateConstraintIndexes?`, `comments?`, `partitions?` | `{ tab, options }`, kept with the tab |
| `db.object.readData` | `tab?` | `{ tab, connection, statement, query?, connecting }`: reads the rows again in the background, connecting first when needed (code `passwordNeeded` without a stored password); the previous rows are freed |

The sections: tables have `columns`, `constraints`, `foreignKeys`, `indexes`, `triggers`,
`references` (other tables' foreign keys to it), `dependencies` (what it uses and what uses it),
`rules`, `policies`, `partitions`, `statistics` and `permissions`; foreign tables `columns`,
`constraints`, `triggers`, `statistics`, `permissions`; views `columns`, `source`, `dependencies`,
`rules`, `triggers`, `permissions`; materialised views `columns`, `source`, `indexes`,
`dependencies`, `statistics`, `permissions`; a schema-level index, an extension, an event trigger, a
tablespace or a role only `settings` (their tabs have only the Properties page); functions and procedures `source`, `permissions`; sequences
`settings`, `permissions`; types `settings`, `columns`; schemas `objects`. `db.result.refresh` on a
tab's result reads it again as `db.object.readData` does.

### Structure editing

The Properties page of an object tab edits structure: changes are **staged** on the
tab and accumulate, nothing runs until the person saves; Save shows the script in the connection's
dialect (ADR-0170) with Execute, Copy, Open in Console, Save as Migration and Cancel. A change is a
JSON object with `action`: `createSchema` {name}, `renameSchema` {name, to}, `dropSchema` {name,
cascade}, `commentSchema` {name, comment}, `createTable` {schema, name, columns: [{name, type,
notNull, default, identity, comment}], primaryKey, comment}, `createView` {schema, name, query},
`createSequence` {schema, name, settings: {start, increment, min, max, cycle}}, `createRoutine`
{schema, name, definition (the whole `CREATE FUNCTION`/`PROCEDURE`)}, `rename` {schema, kind, name,
to}, `drop` {schema, kind, name, cascade}, `comment` {schema, kind, name, comment}, `addColumn`
{schema, table, column}, `alterColumn` {schema, table, column, patch: {name, type, notNull, default,
comment}} (an empty default or comment drops it), `dropColumn` {schema, table, column, cascade},
`addConstraint` {schema, table, constraint: {name, kind (`primaryKey`, `unique`, `check`,
`foreignKey`), columns, check, references: {schema, table, columns, onDelete, onUpdate}}},
`dropConstraint` {schema, table, name, cascade}, `createIndex` {schema, table, index: {name,
columns, unique}}, `dropIndex` {schema, table, name}. `kind` is an object kind (`table`, `view`,
`materializedView`, `foreignTable`, `function`, `procedure`, `sequence`, `type`, `event`); a
routine's `name` carries its arguments as its path does. Every change is checked when staged
against what the server can do, and refused with code `unsupported` and `refusal: { change, reason,
subject }`: `driver`, `schemas` (SQLite), `renameSchema` (MySQL, MariaDB), `sequences` (MySQL,
SQLite), `routines` (SQLite), `comments`, `rename`, `cascade` (only PostgreSQL has it),
`identity`, `constraintKind` (exclusion), `generated`, `restate` (a MySQL column with `ON UPDATE`,
`INVISIBLE`, `SRID`), `rebuildLoses` (SQLite: `CHECK`, `GENERATED`, `AUTOINCREMENT`, `COLLATE`,
`STRICT`, `WITHOUT ROWID`, triggers of an attached schema), `runFirst`, `unknownSchema`,
`unknownObject`, `unknownColumn`, `unknownConstraint`, `unknownIndex`, `exists`, `missing`,
`notOneStatement` (SQL written into a change must be one statement or expression), `notEmpty`,
`action`, `nothing`.

| Command | Parameters | Answers |
|---|---|---|
| `db.structure.stage` | `tab?`, `change` | the state below; refused with `unsupported`, `readOnly` (read-only connection), `loading` (the table's details are being read; try again), `saving` (the script runs) |
| `db.structure.state` | `tab?`, `limit?` (200) | `{ tab, connection, changes: [{ index, summary, change }], total, truncated, dirty, running?, last?: { status, query, message? }, readOnly, confirm, supports: { structure, schemas, renameSchemas, sequences, routines, comments, cascade, rebuilds }, dialect? }` |
| `db.structure.unstage` | `tab?`, `index` | the state; refused with `unsupported` when a later change needs this one |
| `db.structure.discard` | `tab?` | the state, nothing staged; nothing is sent |
| `db.structure.preview` | `tab?`, `limit?` (200) | `{ sql, statements, total, truncated, notes, transaction, confirm }`: the script as it runs; PostgreSQL and SQLite in auto-commit wrap it in `BEGIN` … `COMMIT`; notes say when SQLite rebuilds a table, when MySQL commits each statement and when manual commit leaves it to Commit |
| `db.structure.execute` | `tab?`, `confirmed?` | `{ query?, statements }`: runs the whole script (a `skip` is refused with code `allOrNothing`; its `confirmExecute` data says `skippable: false`) in the background in the Main context; a SQLite rebuild reads `PRAGMA foreign_keys` first, switches foreign keys off for it and on again when they were on, and is refused with code `rebuildInTransaction` outside auto-commit or with a transaction open; stopping at the first error (rolled back inside the transaction); production safety as `db.query.run` (`readOnly`, `agentReadOnly` with `handOff`, `agentNotAllowed`, `confirmExecute` with `token` and `retry`, which every `DROP` and `ALTER … DROP` asks); afterwards the changes are cleared and only the changed objects (or the schema of a created, renamed or dropped one) are read again, or they stay staged with `last.message` |
| `db.structure.toConsole` | `tab?` | `{ path, connection, tab }`: a new console with the script, its notes as comments; the changes stay staged |
| `db.structure.saveMigration` | `tab?`, `folder?` (`migrations`), `name?` | `{ path, statements }`: a new file `<folder>/<UTC yyyymmddhhmmss>_<name>.sql` in the project, never over an existing file nor outside the project (`invalidFolder`) |

Closing a tab with staged changes is refused with code `pendingStructureChanges` and `{ tab, retry }`
(the window asks to discard them). The statements land in the query history with source
`structure`.

### The value panel and Calc

The value panel beside every result grid (consoles' and object tabs'; ⇧⌘P,
`db.result.toggleValuePanel`) shows the selected cell; the Calc part under it, and the status bar
when more than one cell is selected, show the figures of the selection. Long values are rendered
and pictures decoded off the window's thread.

| Command | Parameters | Answers |
|---|---|---|
| `db.result.value` | `id`, `row`, `column`, `format?`, `limit?` (4000, ≤ 100 000 characters) | `{ id, row, column, name, type, kind, null, formats, format, text, length, truncated, size?, picture?: { format, bytes } }`: `formats` are what the value offers, its preferred one first: `text` (arrays as their SQL literal, `{1,"a b",NULL}`; timestamps as the server sent them, with the zone), `json` (pretty, keys in order, for JSON, documents, maps, arrays and text holding JSON), `xml` (indented), `hex` (a dump of bytes or of text's UTF-8), `image` (bytes holding a PNG, JPEG, GIF, WebP, BMP or ICO), `geometry` (spatial values as WKT, `SRID=…;` first, with `geometry: { kind, srid?, dimensions, encoding, points, parts, empty, bounds? }`); without `format`, the setting `valueFormat` when offered |
| `db.result.saveValue` | `id`, `row`, `column`, `path`, `format?`, `overwrite?` | `{ path, bytes }`: bytes as they are, text as UTF-8, or rendered in `format`; an existing file is refused with code `exists` unless `overwrite`, a folder with `isFolder` |
| `db.result.calc` | `id`, `rows?` (`[first, last]`), `columns?` | `{ id, cells, count, numeric, sum?, avg?, min?, max?, distinct, truncated }`: integers summed exactly; `min` and `max` numeric when there are numbers, else as text; at most a million cells |
| `db.result.geometry` | `id`, `rows?` (`[first, last]`), `columns?`, `limit?` (100 shapes, ≤ 1000), `chars?` (1000 WKT characters each, ≤ 100 000; 0 leaves the WKT out) | `{ id, shapes: [{ row, column, kind, srid?, dimensions, encoding, points, parts, empty, bounds?, wkt?, truncated }], bounds?, cells, truncated }`: the spatial values the geometry viewer draws — PostGIS `geometry` and `geography` (EWKB), MySQL geometry, GeoPackage blobs, WKB, WKT and EWKT text, GeoJSON; `encoding` is `wkb`, `ewkb`, `mysql`, `geopackage`, `wkt`, `ewkt` or `geojson`; at most 100 000 cells; nothing is fetched from the network |
| `db.result.toggleValuePanel` | `shown?` | `{ shown, format? }`; kept as the settings `valuePanel` and `valueFormat` (`db.settings.update { valueFormat: null }` goes back to each value's own) |

### Filtering, sorting and editing data

Every data view (an object tab's Data page, a console result) has a WHERE filter and staged edits
([database-workspace.md](database-workspace.md#filtering-sorting-and-editing-data)).
Every command takes `id?`: the result; without it, the result the Database workspace shows in front
(the active object tab's rows, or the active console's result tab; code `noResult` otherwise). A
filter or sort reads the rows again in the background in the same session; the new result replaces
the old one in its tab once its columns have come (its id changes: read `id` from the answer or
`db.object.state`); a result whose columns have not come yet is refused with code `columnsPending`,
staging edits while the rows are read again with code `reading`. Whatever would drop staged edits
(reading the rows again, a console run or refresh replacing unpinned result tabs, `db.result.close`,
`db.console.close`, `workspace.tab.close` of a console or object tab) is refused with code
`pendingChanges` and data `{ results, retry: { command, params } }`: save (`db.result.save`) or cancel
(`db.result.revert`) those results, then run `retry`; the window asks Save, Discard or Cancel. Answers never
hold cell values except the ones asked for (`db.result.distinctValues`) and never parameter values.

| Command | Parameters | Answers |
|---|---|---|
| `db.result.filterState` | `id?`, `limit?` (50) | `{ id, condition, sorts: [{ column, name, descending }], clientSort, applied?, composition? (table, merged, wrapped, unchanged), filterable, query?, error?, canGoBack, canGoForward, history, saved: [{ name, condition }], filteredColumns }` |
| `db.result.filter` | `id?`, `condition` (empty clears) | the filter state; refused with code `invalidFilter` (a `;`, unbalanced parentheses, an unclosed quote or comment, or a statement that does not only read) or `pendingChanges` |
| `db.result.select` | `id?`, `row` (from 1), `column?` | `{ id, row, rows }`: Go to Row; the window selects the row in the grid and scrolls to it; code `noRow` past the rows read |
| `db.result.sort` | `id?`, `column`, `direction?` (`asc`, `desc`, `none`; else the next in that cycle), `add?` | the filter state; in memory when every row is loaded (`clientSort`), else read again with ORDER BY; code `cannotSort` |
| `db.result.filterByValue` | `id?`, `column`, `operator` (`equal`, `notEqual`, `greater`, `less`, `contains`, `isNull`, `isNotNull`, `in`), `row?`, `rows?`, `value?` | the filter state; the condition added with AND |
| `db.result.resetFilter` | `id?`, `column?` | the filter state; all, or the AND parts naming the column; code `invalidFilter` when the result's statement may change data, which never runs again |
| `db.result.filterBack`, `.filterForward` | `id?` | the filter state |
| `db.result.completeFilter` | `id?`, `text`, `caret?`, `limit?` (50) | `{ start, end, items: [{ label, insert, kind (column, keyword), detail? }], truncated }` |
| `db.result.distinctValues` | `id?`, `column`, `limit?` (50, ≤ 1000) | `{ values: [{ text?, row, count }], truncated, partial }`, from the rows loaded |
| `db.result.find` | `id?`, `text`, `matchCase?`, `limit?` (200) | `{ cells: [[row, column]], truncated }`, in memory |
| `db.filter.save`, `db.filter.delete` | `id?`, `name`, `condition?` | the filter state with the table's saved filters (`<data folder>/databases/filters.json`) |
| `db.result.editState` | `id?` | `{ id, editable, reason? (connection, driver, noRowIdentity, noTable, severalTables, unknownTable, noKey), message?, documents? (true for documents of a collection, saved as MongoDB shell statements), target? (the collection's shell expression), table?, key?: { kind (primaryKey, uniqueKey, rowId, virtual, allColumns), columns, names }, columns: [{ index, name, type, editable, nullable, default?, sourced }], pending: { cells, updated, inserted, deleted }, dirty, confirm, saving?, lastSave?: { status, report?, message? } }` |
| `db.result.setCell` | `id?`, `row`, `column`, `text?`, `null?`, `default?` | the editing state; codes `readOnly` (with `reason`), `readOnlyColumn`, `notNull`, `invalidValue`, `noCell` |
| `db.result.revertCell` | `id?`, `row`, `column` | the editing state; refused with code `saving` while the result's save runs |
| `db.result.addRow` | `id?`, `copyOf?` | `{ row, …editing state }` |
| `db.result.duplicateRow` | `id?`, `row` | `{ row, …editing state }`; key and identity columns keep their defaults |
| `db.result.deleteRows` | `id?`, `rows`, `restore?` | the editing state; `restore` is refused with code `saving` while the result's save runs |
| `db.result.copyFromAbove` | `id?`, `row`, `columns?` | the editing state |
| `db.result.previewChanges` | `id?`, `limit?` (200) | `{ sql (values as literals), statements: [{ kind, row, sql (placeholders), preview }], total, truncated, confirm }` |
| `db.result.save` | `id?`, `confirmed?` | `{ operation?, statements }`; code `confirmDataChange` with `{ preview, statements }` where the connection type confirms data changes (an agent confirms only where the type gives agents `full` access, else `agentReadOnly`); a read-only connection or type refuses it; `db.result.editState` then gives `lastSave` (a conflict names the row) |
| `db.result.revert` | `id?` | the editing state, nothing staged; refused with code `saving` while the result's save runs |
| `db.result.setRowKey` | `id?`, `key` (`{ kind: columns, columns }` or `{ kind: allColumns }`), `remember?` | the editing state; a remembered key goes to the project's `.zen/databases/virtual-keys.json` |
| `db.result.generateSql` | `id?`, `kind` (`select`, `insert`, `update`, `delete`), `rows` (`[first, last]`) | `{ sql, rows, truncated }` (≤ 1000 rows) |
| `db.result.paste` | `id?`, `text?` (else the clipboard's), `row`, `column` (the anchor), `mode?` (`overwrite`, `insert`), `delimiter?`, `header?` (a header naming the result's columns puts each value under the column it names, `column` then ignored), `nullText?`, `dryRun?`, `skipInvalid?`, `limit?` (50 problems), `preview?` (≤ 20 rows) | `{ delimiter, header, rows, columns, cells, updatedRows, newRows, skipped, problems: [{ line, column, name, code (invalidValue, readOnlyColumn, notNull, noColumn, deletedRow), message }], problemsTotal, truncated, applied, preview?, pending, dirty }`; staged only; codes `invalidPaste` (with `problems`), `emptyPaste`, `tooLarge` (> 100 000 cells), `readOnly` |

### Following foreign keys

A result whose rows come from one known table (an object tab's Data page, or a console result whose
columns the server traces to one table) links its foreign-key columns to the rows they name, and its
rows to the rows of other tables that point at them, as the connection's snapshot knows the keys
([database-workspace.md](database-workspace.md#following-foreign-keys)). Following a
key opens the other table's Data page as an object tab filtered by the key (`a = 1 AND b = 'x'`, in
the driver's dialect, through `db.result.filter`'s machinery, so its history and Back hold it) and
reads it in the background in the Main context. Answers never hold cell values, except the lookup's
rows, which were asked for; a key with a NULL is refused with code `nullKey`.

| Command | Parameters | Answers |
|---|---|---|
| `db.result.relations` | `id?`, `limit?` (50) | `{ id, table?, links: [{ name, columns, names, table, otherColumns, path? }], references: [same], reading, truncated }`; `reading` while the table's details are read |
| `db.result.openReferenced` | `id?`, `row`, `column?` or `key?` | `{ tab, connection, path, condition, query?, canGoBack, canGoForward }`; codes `noLink`, `nullKey`, `noRow`, `structurePending`, `pendingChanges` |
| `db.result.openReferencing` | `id?`, `row`, `table` (`schema.table` from `references`), `key?` | the same |
| `db.data.back`, `db.data.forward` | — | `{ canGoBack, canGoForward, back, forward, tab?, query? }`: the tab before (or after) with the filter it had; code `tabClosed` for a closed console |
| `db.data.navigation` | — | `{ canGoBack, canGoForward, back, forward }` (titles, nearest first, ≤ 20) |
| `db.lookup.start` | `id?`, `row`, `column` (of a foreign key), `search?`, `limit?` (200, ≤ 200) | the lookup state; reads `SELECT * FROM <referenced> [WHERE key or text columns LIKE '%search%'] ORDER BY <key> LIMIT limit+1` in the background; codes `noLink`, `readOnly`, `readOnlyColumn`, `noRow` |
| `db.lookup.state` | — | `{ open, id?, row, columns, table, key, search, headers, rows: [[text?]], status?, query?, error?, truncated }` (values cut to 200 characters, the key first, ≤ 8 columns) |
| `db.lookup.choose` | `row` (of the lookup) | the editing state: the key staged into every column of the foreign key; the lookup closes |
| `db.lookup.close` | — | `{ closed }`; cancels its read and frees its rows |

Keys in the Database workspace: ⌘S Save, ⌥⌘Z Cancel, ⌃⌥⇧T or ⌥⌘F the filter box (`view.database.focusFilter`),
⌃⇧C or ⇧⌘C Advanced Copy (`view.database.copyAdvanced`), ⌃⇧V or ⌥⇧⌘V Advanced Paste (`view.database.pasteAdvanced`;
`get_window_state` lists the dialogs as `databaseAdvancedCopy`, `databaseAdvancedPaste`); ⌘C and ⌘V
copy and paste in a focused grid;
in a focused grid: ↩ edits the cell, Esc drops its change, ⌘N, ⌘D and ⌘⌫ add, duplicate and delete
rows, ⌘B opens the row a key cell references and ⇧↩ chooses a key cell's value from the referenced
table (`get_window_state` lists the list as `databaseKeyLookup`).

### Bind parameters and script variables

Placeholders are found outside strings, comments, quoted names and dollar quotes
(`zen_db::params`): `:name` in every SQL dialect, `$1` in PostgreSQL and standard SQL (SQLite), `?` and
`?1` elsewhere (in PostgreSQL `?` is a JSON operator); a value is given under the placeholder's
name (`$1`, `?1`, `id`), the n-th plain `?` as `?n`. They are sent as **real bind parameters**: rewritten to `$1…` for PostgreSQL
(a name used twice binds once) and to `?` for MySQL and MariaDB (prepared statements with typed
values, so `LIMIT ?` takes an integer); SQLite (the `sqlite3` program) and ClickHouse have no
binding, so there the values are written into the text as the dialect's quoted literals
(ADR-0170) and the run's output says so (code `parametersInlined`). A value is a string, number,
boolean or `null`, or `{"type": "text" | "number" | "boolean" | "null", "value": "…"}`; a number or
boolean that does not read as one is refused with code `invalidParameter`. Without `parameters` a
console run with placeholders is refused with code `parametersNeeded`: `{ parameters: [{ name,
kind, value?, secret, inferred }], variables: [{ name, value?, secret }], inlined, retry }`, the
window's Bind Parameters dialog; with `parameters` (even `{}`) the values the console remembers fill
the rest. `inferred` is the type the placeholder's place suggests (`zen_db::params::infer_types`):
`number` next to a number, in arithmetic, after `LIMIT`, `OFFSET`, `TOP` or `FETCH FIRST`, or
compared (`=`, `<`, `IN (…)`, `BETWEEN`) with or assigned to a numeric column of the connection's
cached structure, `boolean` next to `true`/`false` or a boolean column, the type of a cast
(`$1::int`, `CAST(? AS …)`), else `text`; the dialog starts with it unless a value is remembered,
whose own type wins. `db.console.explain` and `db.script.runOnConnections` take the same step: the
same refusal with their own `retry`, and `parameters` and `variables` given with them. Explain binds
on the server (PostgreSQL's extended protocol, MySQL's and MariaDB's prepared statements) and
answers `inlined` (`:name = literal`) where a driver writes literals; a run on several connections
asks once for every connection (each placeholder and variable once) and answers each target's
`inlined`.
`${name}` variables are text written into statements before they run (a table name, a schema);
`@set name = value` lines set them in order, `@include file.sql` runs another file's statements in
place (next to the script, else in the project; eight levels at most, never itself). Variables and
values given are remembered per console or script in `<data folder>/databases/variables.json`,
except secret-looking names (`password`, `token`, `secret`, `api_key`…), which stay in memory only
and are never answered. The confirmation of a run (`confirmExecute`) lists each statement's
`parameters` and its token covers the values.

| Command | Parameters | Answers |
|---|---|---|
| `db.console.parameters` | `path?`, `scope?` (`statement`, `script`, `fromCaret`), `offset?` | `{ parameters: [{ name, kind, value?, secret, inferred }], variables: [{ name, value?, secret }], inlined }`: what a run would ask |
| `db.console.variables` | `path?`, `limit?` (200) | `{ path, variables: [{ name, value?, secret }], parameters: [{ name, value?, secret }], truncated }` |
| `db.console.setVariable` | `path?`, `name`, `value?` (absent removes it) | the same |
| `db.console.setParameter` | `path?`, `name`, `value?`, `forget?` | the same |

### Query history

Every statement the engine runs for a window is recorded: console statements and scripts, filters and sorts that read rows again, object tabs'
Data pages, saved data edits, Commit and Rollback, and `db.query.run`, each with its time,
connection, context, schema, duration, rows read or changed, outcome and error, and `agent` when the
command came from an agent. The engine hands each statement to the history (`zen_db::history`) from
its runtime; a thread of its own appends it as a JSON line to `<data folder>/databases/history.jsonl`
(a line torn by a crash is dropped when read) and keeps the statements in memory for searching.
A statement with bind parameters is kept as sent (`$1`, `?`) with `parameters`: their names, and
`:name = value` only when `parameterValues` is on (off by default), values of
secret-looking names (`password`, `token`, `secret`, `api_key`…) always `***`; passwords written in statements (`PASSWORD '…'`,
`IDENTIFIED BY …`, `password=` in connection strings, a URI's password, Redis `AUTH`, `HELLO … AUTH`,
`CONFIG SET requirepass`, `ACL SETUSER … >…`) are masked as `***` before they are kept. Statements
older than the retention or past the limits (10 000 statements, 64 MiB of text; one statement at
most 64 KiB) are dropped, and the file is written again atomically when dead lines pile up. The
connection's id is kept, so a removed connection's statements stay with their id.

| Command | Parameters | Answers |
|---|---|---|
| `db.history.list` | `query?` (words, all must match, any case, text or error), `connection?`, `sources?` (`statement`, `script`, `filter`, `data`, `save`, `transaction`, `query`, `export`, `import`, `session`, `structure`, `connections`, `search`), `agent?`, `outcome?` (`done`, `failed`, `cancelled`, `refused`), `period?` (`today`, `week`, `month`), `since?`, `until?` (ms since 1970), `offset?`, `limit?` (50, ≤ 500), `textLimit?` (500) | `{ entries: [{ id, at, time, connection, connectionName?, context, schema?, text, textCut, redacted, durationMs, rows?, affected?, outcome, error?, code?, source, agent, confirmed, place?, parameters? }], total, truncated, kept, enabled, loading, revision, error? }`, newest first |
| `db.history.get` | `id` | one entry with its whole text; code `unknownEntry` |
| `db.history.openInConsole` | `id`, `run?` | `{ path, connection, run? }`: a new console of the entry's connection with its text, run at once with `run` (refused with code `notWhole` for a statement kept cut or masked) |
| `db.history.open` | `query?`, `connection?` | the Query History tab of the Database workspace, brought to front with that search |
| `db.history.clear` | `connection?`, `before?` (ms) | `{ kept, bytes, settings, file }` after removing them |
| `db.history.settings` | `enabled?` (the off switch), `retentionDays?` (90, 1–3650), `maxEntries?` (10 000, 100–100 000), `parameterValues?` (keep values of bind parameters; off) | the same; kept in `<data folder>/databases/history-settings.json` |
| `db.recovery.list` | `limit?` (50, ≤ 500) | `{ items: [{ id, kind (`console`, `edits`, `transaction`), at, time, title, connection?, connectionName?, path?, count, modifying? }], total, truncated, loading, revision, file, error? }`, newest first: what an earlier run kept and left when it stopped without closing |
| `db.recovery.get` | `id`, `textLimit?` (2000) | the item with `text` (a console's text, or the SQL that would save the edits, values as literals), `textCut`, `statement?`, `log` (a transaction's last statements, passwords masked); code `unknownItem` |
| `db.recovery.restore` | `id` | `{ path, tab }`: a console opens with the recovered text as unsaved changes; edits open as SQL in a new console of their connection, not run; a transaction is refused with code `rolledBack` |
| `db.recovery.discard` | `ids?`, `all?`, `confirmed?` | `{ discarded, left }`; nothing without `ids` or `all: true`; unsaved text or edits among them refuse with `confirmDiscard` (`items`, `retry`) until `confirmed: true`; the journal is deleted when it keeps nothing |
| `db.recovery.open` | | the Recovered tab of the Database workspace |

### Explain plans

Explain Plan (⌃⇧E, also ⌥⌘E) opens a Plan tab (`kind: "plan"`, id `plan:<context>`) in the Database
workspace. A plain explain never runs the statement; with `analyze` it runs inside a transaction (a
savepoint in an open one) that is rolled back, and is checked as running it would be (read-only,
`confirmExecute` with a `token`, `agentReadOnly`, `agentNotAllowed`).

| Command | Parameters | Answers |
|---|---|---|
| `db.console.explain` | `path?` (the active console), `tab?` (a Plan tab to explain again; the one in front when nothing else is given), `statement?`, `analyze?`, `verbose?`, `costs?` (on), `buffers?`, `settings?`, `confirmed?`, `parameters?`, `variables?` ([Parameters](#bind-parameters-and-script-variables)) | `{ tab, operation, connection, statement (as sent, placeholders rewritten), executes, rolledBack, ignored, inlined, connecting }` at once; code `unsupported` for a driver without plans, `noStatement`, `oneStatement` (the text holds several), `analyzeChanges` (ANALYZE of a statement that changes something where it cannot be undone: MySQL and MariaDB, or a database without transactions), `noConnection`, `parametersNeeded` (as db.console.execute), `invalidParameter`; `db.plan.state` answers `inlined` too |
| `db.plan.state` | `tab?`, `offset?`, `limit?` (100, ≤ 1000), `details?`, `raw?`, `rawLimit?` (20 000) | `{ tab, connection, path?, statement, statementCut, status` (`running`, `done`, `failed`, `cancelled`, `notRun`)`, operation?, error?, errorCode?, options, supported, ignored, executes, rolledBack, measure?` (`time`, `cost`, `none`)`, totalCost?, totalTimeMs?, planningMs?, executionMs?, nodes: [{ id, parent?, depth, node, entity?, condition?, startupCost?, cost?, selfCost?, rows?, actualRows?, loops?, timeMs?, selfTimeMs?, share, hot, children, details }], total, truncated, hot, selected?, planDetails, raw?, rawTruncated, view }` |
| `db.plan.select` | `tab?`, `node` | the node with its details, shown in the details pane; code `noNode` |
| `db.plan.view` | `tab?`, `view` (`tree`, `raw`) | `{ tab, view }` |

### Finding database objects

`db.object.goTo` is Go to Database Object (⌘O in the Database workspace, `view.gotoDatabaseObject.open`,
which opens Search Everywhere on its **Database** tab) and that tab's search; the tab is also a group
of All (`search.everywhere.query { tab: "database" }`). Names come from every connection's snapshot,
read from disk when needed, never from a server: nothing connects. The names are kept as one flat
list per window, built again when a snapshot changes, and matched with the platform's name matcher
(as Go to Class: camel humps, `*`, a space for a word start); the popup matches on a thread of its
own, cancelled by the next keystroke. Objects come before columns of the same match; then the
connections' scripts and consoles by file name, then statements of the query history holding every
word of the query. System objects and schemas are left out.

| Command | Parameters | Answers |
|---|---|---|
| `db.object.goTo` | `query` (`name`, or `qualifier.name`: a schema before an object, a table before a column), `limit?` (50, ≤ 500), `connection?`, `kinds?` (`table`, `view`, `materializedView`, `foreignTable`, `function`, `procedure`, `sequence`, `type`, `event`, `schema`, `column`, a driver's own `key` or `collection`, `script`, `console`, `history`) | `{ items: [{ name, kind, connection, connectionName, detail, path?, file?, id?, matchKind?, ranges, command, params }], total, truncated, loading, names }`: `detail` is the schema (`catalog.schema` when the connection lists several databases) or `schema.table` for a column, a script's place; `command` and `params` open the item (`db.object.open`, a column on its table's Properties; `workspace.tab.open` for a script; `db.history.openInConsole` for a statement); `loading` while a snapshot is read from disk; `names` is how many names were searched |

Search Everywhere's items of this tab have the target `{ kind: "run", command, params, icon }`;
choosing one runs `command`, which expands the Database workspace.

### Sessions and locks

The server's sessions of a connection: PostgreSQL from
`pg_stat_activity` (blockers by `pg_blocking_pids`) and `pg_locks`, MySQL from the process list and
`performance_schema.data_lock_waits` / `data_locks`, MariaDB from the process list and
`information_schema.INNODB_LOCK_WAITS` / `INNODB_LOCKS`. Other drivers answer code `unsupported`
(`Capabilities::sessions`). Everything runs in the connection's Metadata context in the background;
nothing connects by itself except the explicit list, cancel and terminate commands (with the stored
password, as `db.key.get` does). Queries are cut to `textLimit` and their passwords masked as the
query history masks them (`redacted`). This IDE's own sessions are marked `own` with their
`context`; their ids come from the drivers (`pg_backend_pid()` read with the guards, MySQL's
connection id).

Cancelling a query and ending a session always ask first, on every connection type: the first call
is refused with code `confirmExecute` (the shape of ADR-0172's refusal: `token`, `target`, the one
statement it sends with its `risks`, `session` as last listed, `retry`); calling again with
`confirmed: token` sends it, and `skip: [0]` sends nothing (code `skipped`). The token is bound to
the connection, the action and the session. One of this IDE's own sessions is refused with code
`ownSession` unless `own: true`. A read-only connection refuses both (`readOnly`, also in the
engine); agents get `agentNotAllowed` where they may run nothing and `agentReadOnly` with a
`handOff` to `db.session.open` where they only read (Production). Every statement sent, and every
refusal, is in the query history with source `session`.

| Command | Parameters | Answers |
|---|---|---|
| `db.session.list` | `connection`, `showIdle?` (true; idle-in-transaction and blocking sessions are always listed), `locks?` (true), `limit?` (100, ≤ 1000), `textLimit?` (500, ≤ 10 000), `waitMs?` | `{ operation, status, connection, sessions?: { sessions: [{ id, user?, host?, database?, application?, command?, state?, queryStart?, seconds?, query?, queryCut, redacted, wait?, blockedBy, blocking, own, context? }], total, truncated, locks: [{ session, kind, mode, granted, object?, blockedBy }], locksTruncated, locksError?, chains }, message?, error? }` |
| `db.session.state` | `operation`, `waitMs?` | the same for a listing, or `ended: { statement, ended }` for a cancel or terminate |
| `db.session.cancelQuery` | `connection`, `session`, `own?`, `confirmed?`, `skip?`, `waitMs?` | asks first (`confirmExecute`), then `pg_cancel_backend` / `KILL QUERY`; `ended.ended` false when the server did not find the session |
| `db.session.terminate` | `connection`, `session`, `own?`, `confirmed?`, `skip?`, `waitMs?` | asks first, then `pg_terminate_backend` / `KILL CONNECTION`; its open transaction is rolled back |
| `db.session.open` | `connection`, `showIdle?`, `autoRefresh?` (seconds, 0 = on demand) | the Sessions tab (kind `sessions`, id `sessions:<connection>`), brought to front with those settings |

### Command-line clients and code types

A connection's own command-line client runs in a new tab of the Terminal tool window
([database-workspace.md](database-workspace.md#command-line-clients-and-code-types)):
psql, mysql or mariadb, sqlite3, redis-cli or valkey-cli, clickhouse client, found on `PATH` and in
the folders installers use (Homebrew, its keg-only `libpq` and `mysql-client`, Postgres.app). The
password is never an argument, never in the client's environment (which other programs of the user
can read) and never on the screen: psql, mysql, mariadb and clickhouse client read it from a new
file only the user can read (`PGPASSFILE`, `--defaults-extra-file`, `--config-file`), removed when
the client ends or its tab closes; redis-cli asks for it (`--askpass`), and so does every client on
Windows. Agents may open a client only where their access is full and the connection is neither
read-only nor asks before changes (a client runs whatever is typed into it), and never with a saved
password or the connection's (they can read the terminal). Nothing connects the connection itself.

| Command | Parameters | Answers |
|---|---|---|
| `db.connection.openTerminal` | `connection`, `password?` (this time only, never stored) | `{ tool, program, session?, operation?, passwordFrom }`: `passwordFrom` is `given`, `connection` (the connected connection's), `saved` (read in the background, operation kind `readSecret`; the client starts when `operation` ends and `session` is null until then) or `asked`; codes `clientNotFound`, `noClient` (no client for the driver, an SSH tunnel not open — connect first —, ClickHouse through SSH, Redis Sentinel, SQLite without a file), `agentNotAllowed` |
| `db.connection.clientCommand` | `connection` | `{ tool, args, env, passwordVia?, program?, commandLine }`: the command for a POSIX shell, with the non-secret variables (TLS); `passwordVia` is how the client gets a password (`PGPASSFILE`, `--defaults-extra-file`, `--config-file`, `--askpass`), never the password |
| `db.object.generateCode` | `connection`, `path` (a table, foreign table, view or materialized view), `language` (`go`, `rust`, `typescript`) | `{ typeName?, code?, fields, truncated, loading, operation? }`: a Go struct (`db` and `json` tags, pointers for nullable columns), a Rust struct (`Option` for nullable columns) or a TypeScript interface (`\| null`), named after the table made singular, at most 1000 fields; offline from the snapshot, `loading` while the details are read; codes `noColumns`, `notRead`, `unknownObject` |
### ER diagrams

A schema's ER diagram is a tab of kind `diagram` (id `diagram:<connection>:<catalog>/<schema>`); a
table's or view's is its object tab's Diagram page. Diagrams come from the connection's snapshot,
offline, and are laid out in the background; nothing connects
([database-workspace.md](database-workspace.md#er-diagrams)). Every command takes
`tab?` (a diagram tab or an object tab with a Diagram page; the active tab when absent), else code
`noDiagram`.

| Command | Parameters | Answers |
|---|---|---|
| `db.diagram.open` | `connection`, `path` (`[catalog, schema]` or `[catalog, schema, folder, name]`) | `{ tab, of }` (`schema` or `table`); expands the workspace; lays out again after a cancel; code `unknownObject` |
| `db.diagram.state` | `tab?`, `offset?`, `limit?` (100, ≤ 1000), `columns?` (true), `waitMs?` (≤ 10 000) | `{ tab, connection, path, status` (`computing`, `ready`, `failed`, `cancelled`, `missing`)`, error?, elapsedMs?, partial, entities: [{ name, schema, kind, path, detailed, bounds? ([x, y, width, height] at 100 %), columnCount, columns?: [{ name, type, keys, nullable }] }], total, relations: [{ name, from, fromColumns, to, toColumns, cardinality, optional }], relationTotal, truncated, size?, view: { zoom, x, y }, selected?, export?: { format, path, status, bytes?, error? } }` |
| `db.diagram.zoom` | `tab?`, `action` (`in`, `out`, `reset`, `fit`), `width?`, `height?` (the canvas in points, 1200 × 800) | the view; steps 10 % to 400 %, the canvas's centre kept; `fit` at most 100 % (code `notReady` before the layout) |
| `db.diagram.view` | `tab?`, `zoom?`, `x?`, `y?`, `dx?`, `dy?` | the view: zoom in percent, the diagram point at the canvas's top-left corner, or moved by `dx`, `dy` |
| `db.diagram.select` | `tab?`, `entity?` (a name as listed, or `schema.name`; none clears), `open?` | `{ entity?, path?, opened? }`; `open` opens its object tab; code `unknownEntity` |
| `db.diagram.export` | `tab?`, `format` (`png`, `svg`, `mermaid`), `path?` (absolute), `overwrite?`, `scale?` (PNG pixels per point, 1 to 4, 2), `limit?` (20 000 characters) | without `path`: `{ format, text, truncated }` (SVG and Mermaid); with it: `{ format, path }` at once, written in the background (follow `db.diagram.state`'s `export`); codes `notReady`, `pathNeeded`, `invalidPath`, `exists`, `noRenderer` (PNG and SVG need an IDE window for the theme) |
| `db.diagram.cancel` | `tab?` | `{ cancelled }`: stops the layout and the export in flight |

### Migrations

The project's migration tools ([database-workspace.md](database-workspace.md#migrations)):
folders recognised from their files in the background (Flyway, Liquibase, golang-migrate, goose,
Atlas, sqlx, Diesel, Alembic, Django, Prisma, Rails, Knex), the tool's own table read on a connected
connection, and Up or Down run with the tool as a run configuration (kind `db.migration`) in the Run
window. Nothing connects by itself. The password reaches the tool in its environment and is never in
an answer, the command shown, or a saved configuration (which names the connection only). Running
follows ADR-0172: `readOnly` on a read-only connection; `down` always asks and `up` asks where the
connection type confirms execution (`confirmExecute` with the command as the one statement, the
pending migrations and a `token`; run again with `confirmed: token`); agents get `agentReadOnly`
with a `handOff` to `db.migration.open` where they only read, and `agentNotAllowed` where they run
nothing. When a run ends, the connection's structure is read again (`run.refresh`) and the applied
state too.

| Command | Parameters | Answers |
|---|---|---|
| `db.migration.list` | `limit?` (50), `refresh?` (scan again) | `{ sets: [{ id, tool, toolName, folder, base, config?, app?, count, reverts, table }], total, truncated, scanning }` |
| `db.migration.status` | `set`, `connection?`, `refresh?` (read the table again), `state?` (`applied`, `pending`, `failed`, `unknown`), `limit?` (200) | `{ set, connection?, connected, readOnly, loading, table?, counts: { applied, pending, failed, unknown, missing }, migrations: [{ version, name, file?, state }], total, truncated, error?, run?: { runId?, direction, state?, exitCode?, waiting, refresh? } }`; a migration without `file` is applied on the database only; `readOnly` tells that db.migration.run is refused there |
| `db.migration.run` | `set`, `connection`, `direction` (`up`, `down`), `confirmed?`, `password?` (this IDE's runs only, never stored), `save?` (also a project run configuration) | `{ runId?, waiting, command, configurationId? }`: `waiting` while the stored password is read; read the output with `run.output.get`; codes `confirmExecute`, `readOnly`, `agentReadOnly`, `agentNotAllowed`, `notTrusted` (an agent, in safe mode), `unsupported` (SSH, other drivers, Prisma down), `needsState` (Django down before the state was read), `nothingApplied`, `unknownMigrations` |
| `db.migration.open` | `connection?`, `set?` | the Migrations tab (kind `migrations`, id `migrations`) |

The `unknownMigrations` refusal names `db.migration.list` to an agent and the Migrations tab to a
person (`database::for_caller`).

### Schema and data compare

Two schemas (of connections, or saved snapshot files) or two tables' rows are compared in the
background ([database-workspace.md](database-workspace.md#schema-and-data-compare)).
Scripts are only ever text: answered, or opened in a new console of the target's connection for a
person to review and run, where production safety applies. Never secrets.

| Command | Parameters | Answers |
|---|---|---|
| `db.compare.schemas` | `source`, `target`: `{ kind: connection, connection, schema, catalog?, refresh? }` (read again first unless `refresh: false`; connects with the stored password when needed) or `{ kind: snapshot, path }`; `options?` (`kinds`, `ignoreComments`, `system`) | `{ id }` at once |
| `db.compare.data` | `source`, `target`: `{ connection, schema?, table }`; `key?` (the source's primary key), `columns?` (those both tables have), `page?` (1000), `kept?` (10 000) | `{ id }` at once; codes `noKey`, `noColumns`, `noTable` |
| `db.compare.state` | `id?`, `waitMs?` (≤ 60 000), `limit?`, `offset?` | `{ compares: [{ id, kind (schemas, data), request (the `source` and `target` as given to `db.compare.schemas` or `.data`), status, elapsedMs, error?, code?, source?, target? (label, connection, driver, catalog, schema, cached, taken), summary? (added, removed, changed, same, partial), objects (kind, name, key, change, items: [{ part, name, change, fields, source?, target? }], partial), data? (pass, counts: sourceRows, targetRows, same, changed, added, removed; key, columns, kept, truncated), rows (change, key, columns, source?, target?), total, truncated }], truncated }`; `added` = only in the source, `removed` = only in the target |
| `db.compare.cancel` | `id` | `{ cancelled }` |
| `db.compare.ddl` | `id`, `kind`, `key`, `limit?` (20 000) | `{ source, target, truncated }`: the object's DDL on both sides, generated offline |
| `db.compare.script` | `id`, `drop?`, `open?`, `limit?` (100 000) | `{ text?, statements, notes, console?, truncated }`: the target's dialect; what only the target has is dropped or deleted only with `drop`, else a `--` note |
| `db.compare.open` | `source?`, `sourceSchema?`, `target?`, `targetSchema?`, `id?` | the Compare tab of the Database workspace; without `id` it shows the window's latest comparison, whoever started it (a `db.compare.schemas` or `.data` call also sets its pickers) |
| `db.compare.saveSnapshot` | `connection`, `schema`, `catalog?`, `path?`, `overwrite?` | `{ path, objects, partial }`: the schema's structure as JSON (default `<data folder>/databases/snapshots/`); codes `exists`, `notRead`, `unknownSchema` |
| `db.compare.snapshots` | `limit?` (50) | `{ snapshots: [{ path, connection, schema, taken, objects }], total, truncated }` |

## Settings

`get_settings` returns every field of `IdeSettings` (`settings.json`), in camel case: `mcpEnabled`,
`mcpPort`, `mcpLanguage`, `openInNewWindow`, `openProjectIn`, `defaultProjectDirectory`, `confirmExit`, `whatsNewOnUpdate`, `showTipsOnStartup`, `tipIndex`, `autosaveOnIdle`, `autosaveIdleSeconds` (1–3600),
`autosaveOnFrameDeactivation`, `language`, `showFoldArrows`, `foldArrows` (`onHover` or `always`), `foldBottomArrows`, `foldFileHeader`,
`foldImports`, `foldDocComments`, `foldMethodBodies`, `foldCustomRegions`, `camelHumps`, `camelHumpsOnDoubleClick`, `gutterRunIcons`, `highlightUsagesAtCaret`, `softWrapFiles`,
`breadcrumbs`, `navigationBar` (`top`, `statusBar` or `hidden`), `navigationBarMembers`, `lineNumbers`, `lineNumbersMode`, `methodSeparators`, `stickyLines`, `stickyLinesMax`, `ligatures`, `clipboardLimit`, `theme`, `reformatOnSave`, `editorConfig`,
`removeTrailingSpaces`, `removeTrailingSpacesOn` (`modified` or `all`), `keepTrailingSpacesOnCaretLine`,
`removeTrailingBlankLines`, `ensureLineBreakAtEnd`, `optimizeImportsOnSave`, `uiFontSize`, `ideZoom`, `editorFont` (`family`, `size`, `lineSpacing`),
`notificationBalloons`, `balloonGroupsOff`, `searchWaitForAll`, `searchTextOnAll`, `bookmarksPopupLineOnly`, `findInFilesLimit`, `findInFilesTooMany` and
`codeStyle` (a language id, such as `go` or `default`, to `useTabs`, `tabSize` and `indent`). A null
is the default (`uiFontSize` null is 13 points); a language missing from `codeStyle` takes the
built-in code style of ADR-0053.

`set_settings` takes the same shape, every field optional, and applies the result at once, saving
it and returning it:

- **Objects merge key by key.** `{"codeStyle": {"go": {"tabSize": 8}}}` changes Go's tab size only:
  Go keeps `useTabs` and `indent`, the other languages and every other setting stay. A language
  without an entry starts from the code style it has now.
- **A null brings back the default**: `{"uiFontSize": null}` is 13 points again,
  `{"editorFont": {"size": null}}` the default size, `{"codeStyle": {"go": null}}` removes Go's entry
  so that the built-in style applies, `{"codeStyle": null}` removes every entry.
- **Unknown keys are errors**, at any depth (`editorFont.weight`, `codeStyle.go.tabWidth`), so that a
  misspelt name is not lost silently; a refused call changes nothing. So are values of the wrong type,
  a `uiFontSize` outside 8–32, an `ideZoom` outside 50–300, an `autosaveIdleSeconds` outside 1–3600, an `mcpPort` below 1024, an `mcpLanguage` that is not `en` or `ru`, an `editorFont` size or line spacing of 0 or
  less, and a `tabSize` or `indent` of 0. The messages are in the language for agents (Fluent, `mcp-settings-*`; see [Language for agents](#language-for-agents)).
- `theme` is read only here: `select_theme` changes it; so are `themeFollowSystem`, `themeDark` and `themeLight`, which `follow_system_theme` changes.
- Code folding (Settings → Editor → General → Code Folding): `showFoldArrows`, `foldArrows` and
  `foldBottomArrows` set the gutter's arrows; `foldFileHeader`, `foldImports`, `foldDocComments`,
  `foldMethodBodies` and `foldCustomRegions` are the General "Fold by default" options, and a
  language module's options are on/off `declaredSettings` by key (`go.folding.ifErrors`,
  `go.folding.oneLineReturns`, `go.folding.oneLineCases`, `go.folding.emptyFunctions`,
  `go.folding.emptyTypes`, `go.folding.formattedStrings`). The command **`editor.folding.options`**
  lists the arrows and every option with its key, label, default and value; `editor.fold.list` gives
  each fold the `option` that folds it when a file opens for the first time (ADR-0114).

`uiFontSize` zooms the whole UI (egui's zoom factor is `uiFontSize / 13`, `IdeSettings::font_zoom`) in
every window, at start and whenever settings are applied; the editor's text keeps the size of
`editorFont`. `ideZoom` (View → Appearance → Zoom IDE, Settings → Appearance → Accessibility → Zoom;
100 by default, presets 70–200, any value from 50 to 300) is a percentage on top of it that scales
the editor too: egui's zoom factor is `uiFontSize / 13 × ideZoom / 100` (`IdeSettings::ui_zoom`,
[ADR-0120](adr/0120-ide-zoom.md)). The window actions `view.ideZoom.in` and `view.ideZoom.out` step
through the presets, `view.ideZoom.reset` goes back to 100 and `view.ideZoom.open` opens the Zoom IDE
dialog (popup `zoomIde`); each answers `{ "ideZoom": <percent> }` as its result. None has a default
shortcut. The input schema of `set_settings` (`settings_schema` in `ide/automation.rs`) lists
every field with its range.

## Headless IDE

`<ide> --headless [--mcp-port <port>] [--exit-with-parent] [--accept-open] [--no-restore] [<project>...]` runs the same IDE without
a window ([ADR-0084](adr/0084-headless-ide.md)): the same registry, modules, plug-ins, language
servers, sessions and MCP server, without winit, an event loop or a GPU. Only `IdeApp::logic` runs,
through `egui::Context::run_logic` (no egui pass: nothing is laid out or drawn), woken by repaint
requests and at least every 0.5 s (`zen_ui::ide::run_headless`, `ide/headless.rs`).

- **Port.** `--mcp-port`, else `ZEN_AUTOMATION=<port>`, else a free port. When the server listens,
  one JSON line goes to standard output: `{"url": "http://127.0.0.1:<port>/mcp", "pid": ..., "projects": [...]}`.
- **Instances.** Each one records itself in `<data>/headless/<pid>.json` (`pid`, `url`, `product`,
  `projects`, `started`; `zen_automation::instances`) until it ends; a record whose server no longer
  answers is removed when listed. `automation.json` stays the windowed IDE's, for `<ide> mcp`.
- **Second starts.** `--accept-open` makes it take what a later `<ide> <folder|file[:line[:column]]>`
  on the same data folder hands over, as a windowed IDE does ([ADR-0111](adr/0111-hand-files-to-the-running-ide.md),
  `zen_workbench::handoff`); without it a headless IDE never takes them, so a user's `gogarden
  main.go` never opens in an agent's headless IDE. `ZEN_HANDOFF_ONLY=1` makes such a start that
  nobody takes end with 1 instead of opening a window.
- **A second start** on a project a live headless instance of the same data folder has is refused
  with exit code 3 and a message naming its URL and pid. A bad command line exits with 2.
- **A virtual window**: each project keeps its window state without drawing it,
  so `run_action`, `dispatch_command` with a window action (`view.*`), `get_window_state`,
  `get_tree_selection` and `select_in_tree` work as in a windowed IDE: tool windows open and close
  (Maven, Gradle, Problems, Run…), popups and dialogs open in the state, and the tool windows'
  actions run. Only `press_keys` answers an error at once: "`press_keys` is not available in a
  headless IDE: keys need a drawn window". `view.ide.exit` exits.
- **Exit.** `exit_ide` and `view.ide.exit` exit at once, never asking (`asking` is false): documents
  are saved (when one cannot be, the call fails saying why and the IDE stays), runs, debug sessions
  and background tasks stopped, the language servers shut down with the sessions, the record removed,
  exit code 0. `--exit-with-parent` does the same when the parent process ends (Unix: the parent
  process id changes), and `--exit-when-unused` when its last client left; with nobody left to tell,
  these save every file they can and exit even when one cannot be saved.
- **`--no-restore`** opens the projects without the tabs and tool windows `workspace.json`
  remembers from a windowed IDE (ADR-0139); `<ide> mcp --headless --no-restore` passes it on.
- **What it leaves alone.** The recent projects and which of them open at the next start,
  `workspace.json`, `automation.json` and the system clipboard; it never updates or restarts itself
  and offers no first-start choice of areas. Settings, plug-ins, themes and keymaps are the data
  folder's (`ZEN_DATA_DIR` as usual) and shared with a windowed IDE using it.

`a_headless_ide_serves_problems_usages_and_rename_over_mcp_and_exits` in `zen-ui`
(`ide/headless/tests.rs`) starts one with a scripted language server, and over HTTP lists the
commands, opens a Go file, waits for its problems, finds usages, renames, checks that window-only
tools answer the error, and exits; nothing is laid out (`cumulative_pass_nr` stays 0).

### `mcp --headless`: a headless IDE per agent session

`<ide> mcp --headless [--no-restore] [<project>]` (the current folder by default) is a stdio bridge to a headless
IDE for the project, never to a window ([ADR-0085](adr/0085-headless-ide-per-agent-session.md),
`zen_automation::bridge::run_headless`):

- **Find or start.** It uses the live headless IDE of the same data folder that has the project
  open, or starts `<ide> --headless --exit-when-unused <project>` in its own process group, its
  standard input and output closed and its standard error in `<temp>/<ide>-headless-<pid>.log`.
  Starts are serialised by `<data>/headless/start.lock`: two sessions starting at once get one IDE.
- **Leases.** While it runs, the bridge keeps a file in `<data>/headless/<pid>.clients/` locked;
  the system releases the lock when the bridge's process ends, however it ends
  (`zen_automation::instances::Lease`, `clients`).
- **The last one ends it.** An IDE started with `--exit-when-unused` exits as `exit_ide` does one
  second after its last lease is gone (sixty seconds when none ever came), removing its record
  first. A message that cannot reach the IDE is sent once more to the IDE found or started again.
  An IDE started by `<ide> --headless` or `zen-cli ide start` is used but never ended by bridges.
- **The bridge ends** when its standard input closes, or (Unix) when the process that started it
  ends.

**Restarts.** Both bridges, `<ide> mcp` to the windowed IDE and `<ide> mcp --headless`, live as long
as the agent that started them, so an agent keeps its tools when the IDE restarts (an update, a
crash), whatever the agent is: a call that cannot reach the IDE is sent again once the IDE answers.
`<ide> mcp` waits up to 15 seconds for a restarting IDE to record its server in `automation.json`
before it starts one, and then up to 30 seconds for it; when no IDE comes, the call fails with a
JSON-RPC error that says why, never hanging. `the_same_agent_keeps_its_tools_across_an_ide_restart`
in `zen-automation` restarts the server between two calls of one bridge.

`mcp_headless_starts_one_ide_for_its_sessions_and_stops_it_after_the_last` in `go-garden`
(`tests/command_line.rs`) runs two bridges on one project: one IDE answers both (`tools/list`,
`dispatch_command`, `list_windows`), it survives the first bridge's end and is gone after the last.

### Command line

Every IDE reads its command line before anything starts (`zen_ui::ide::read_command_line`):
`--version`/`-V` prints `<Product> <version>` and `--help`/`-h` the usage, and both exit with 0
without a window, an MCP server or any write to the data folder; a bad command line prints its
usage and exits with 2. These messages follow the `language` of the IDE's `settings.json` when
its data folder has one, else the system's language (`version_and_help_print_and_start_nothing`).

### zen-cli ide

`zen-cli ide` is a thin client of the instance records and the MCP server:

```sh
zen-cli ide start <project> [--ide <program>] [--mcp-port <port>]   # JSON: pid, url, projects, reused
zen-cli ide list [--json]
zen-cli ide call <tool> [--args <json>|-] [--instance <pid|project>]
zen-cli ide stop [--instance <pid|project>]
```

`start` answers the instance that already has the project (`"reused": true`) or starts
`<program> --headless <project>` (`gogarden` by default: a path, a name on `PATH` or the binary of an
application in `/Applications`), waits for its record and answers it with `"log"`, the file its
standard error goes to. `--instance` takes a pid or a project (or a path inside one); left out, the
only running instance. It looks in every IDE's data folder (`ZEN_DATA_DIR`, else the system's).
Exit codes as for the other subcommands: `1` when the IDE answers an error or cannot be started or
found, `2` for a usage error.

## While the window is hidden

Calls are answered on the UI thread, which owns the windows and sessions, but not inside a drawn
frame: `IdeApp::logic` answers them, and the application calls it from `eframe::App::logic`. eframe
runs that logic without drawing while the window is minimised, covered by other windows or hidden
(⌘H), so an agent gets its answer within a fraction of a second (about 0.1 s while minimised or
hidden, eframe's pace for invisible windows) and the window is not brought forward. Each call wakes
the event loop (`request_repaint_of(ROOT)`); a visible window draws as usual, after the logic.

A project's own work advances without a frame too ([ADR-0084](adr/0084-headless-ide.md)):
`IdeView::step` (workbench messages into the notifications, terminals, the Git status and operation,
the language servers' sync and list, usages, runs, the Services requests and debug sessions) runs in
a drawn window's frame, and `IdeApp::logic` runs it for every window that was not drawn since the
last logic, so gopls's problems, Git's status and runs keep up while the window is covered, asleep or
hidden.

Nothing a tool does needs a drawn frame, except `press_keys`, whose keys are input of the next
drawn frame: it waits until the window is drawn again (at most 30 s). What the tools change on screen (an opened document, the
tree's selection) is drawn when the window shows again; `select_in_tree` orders the paths it
returns by the rows of the last drawn frame, so a newly unfolded folder's rows sort last until then.
Settings from `set_settings` apply right after the answer, hidden or not.
Changes other programs make to the project's files are applied in the same logic, drawn frame or
not (`IdeView::follow_disk`, woken by the file watcher): an open file without unsaved changes
reloads, and the tree and version control follow, while the window is covered or hidden too, so
`get_document_text` returns the new text without `file.document.syncWithDisk`.

`answers_agents_without_drawing_a_frame` in `zen-ui` (`ide/app.rs`) checks this headless: it calls
`list_windows` and `dispatch_command` over HTTP while only `egui::Context::run_logic` runs, never a
pass, and requires each answer within a second.

### Checking it by hand

On macOS, with a development build and a data folder of its own, so that nothing of an installed
IDE is touched:

1. `cargo build -p go-garden && export ZEN_DATA_DIR=$(mktemp -d)`, then start it on a free port
   with a project: `ZEN_AUTOMATION=63597 ZEN_RELEASES=none target/debug/gogarden <folder> &`.
2. Minimise its window (or hide it with ⌘H, or cover it completely with another window).
3. With the same `ZEN_DATA_DIR`, call the tools through the bridge and time them: `time (printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_windows","arguments":{}}}' | target/debug/gogarden mcp)`,
   then `dispatch_command` with, say, `file.document.open` and a file of the project.
4. Each answer comes within a second (before the fix it waited 30 s and failed with "the
   application did not answer in time"). Quit the instance.

The bridge starts the installed IDE when the recorded server does not answer, so check that the
development build serves (`automation.json` in the data folder names its `pid`) before calling it.

## Command line (zen-cli)

`zen-cli` runs the same commands without an IDE, a window or a port: a development and automation
tool for agents, scripts and tests, not a released product for now. Its output and exit codes are a
stable interface ([ADR-0042](adr/0042-zen-cli-output-and-exit-codes.md)). From the repository,
run it as `cargo run -p zen-cli -- <arguments>`.

```sh
zen-cli commands
zen-cli commands --json
zen-cli run file.project.open --params '{"path": "/tmp/demo"}'
echo '{"path": "/tmp/demo"}' | zen-cli run file.project.open --params -
zen-cli run --help
zen-cli --version
```

- `commands` prints each command's id and title, sorted by id; `--json` prints
  `{"commands": [...]}` with each command's `id`, `title`, `description`, `paramsSchema` and
  `aliases`.
- `run` takes the parameters as JSON (`{}` when `--params` is left out; `-` reads them from standard
  input) and prints the result as pretty-printed JSON, or `null` when the command returns nothing.
- Errors are one line on standard error, `zen-cli: <message>`. `zen-cli` with no subcommand prints
  the help to standard error and exits with `2`; `--help` (`-h`) and `--version` (`-V`, printing
  `zen-cli <version>`) write to standard output and exit with `0`.
- Help and errors are in English or Russian: `ZEN_LANGUAGE=en` or `ZEN_LANGUAGE=ru` picks one,
  `system` follows the system language (falling back to English); unset, empty or any other value
  is English (ADR-0185).
  Command titles and descriptions stay in English.
- Exit codes: `0` success; `2` a usage error (bad arguments, invalid JSON, unknown command, invalid
  parameters); `1` the command is unavailable, failed, or returned invalid output, standard input
  could not be read, or the output could not be written (for example, a closed pipe).
- The commands are those of `zen-workbench` and `zen-toolbox`. Each run is a fresh session: no
  project is open at the start and nothing persists afterwards. `toolbox.*` commands fail cleanly,
  because the CLI does not set up Toolbox state; it never touches the network, except
  `http.request.run` when it is asked to (give it `waitMs`: the request runs on a thread that ends
  with the process).
