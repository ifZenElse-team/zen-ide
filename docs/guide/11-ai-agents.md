# AI agents

AI agents can work in GoGarden through the Model Context Protocol (MCP): see the windows and open
files, run any action, read problems, use the Terminal, and change settings and the keymap.

## Turning it on

**Settings → Tools → MCP Server → Enable MCP Server.** The server listens on this computer only, at
`http://127.0.0.1:63530/mcp` by default; the page shows the address and the commands to connect an
agent. A badge in the status bar says when it is on.

The simplest way with Claude Code: in your project's folder, run

```sh
claude mcp add -s project gogarden -- /Applications/GoGarden.app/Contents/MacOS/gogarden mcp --headless .
```

Claude Code then starts a GoGarden without a window for this project with each session and it ends
with the session; several sessions on the project share one. Nothing has to be kept open, and your
GoGarden window is never used. See [Without a window](#without-a-window).

With Claude Code, over HTTP (GoGarden must be running):

```sh
claude mcp add --transport http gogarden http://127.0.0.1:63530/mcp
```

Or over standard input and output, which starts GoGarden when needed:

```sh
claude mcp add gogarden -- /Applications/GoGarden.app/Contents/MacOS/gogarden mcp
```

## What agents can do

Everything people can do is an action, and every action is open to agents: `list_commands` lists
them by group (with `group` or `query`, each with the first sentence of its description, whole;
`describe_command` gives one command's parameters), and `dispatch_command` runs one. When an agent
connects, GoGarden tells it the commands it needs most, so it rarely has to look. The most used
ones (replace text, Find in Files, Go to Symbol, Find Usages, a file's problems, rename, run, test
results) are also tools of their own, such as `editor_text_replace`, so an agent sees their
parameters at once.

Working through GoGarden costs an agent fewer tokens than working on its own:

- **One call per edit.** `editor.text.replace` replaces exact text in a file as the editor has it,
  unsaved changes included, as one undo step, and saves it when asked (with Optimize Imports and
  Reformat on save, if they are on). If the text is not there, or is there more than once, nothing
  changes and the agent is told how many times it found it. With `regex` it replaces by a pattern,
  line starts and ends included. `editor.text.write` gives a file its whole text and creates a new
  file; `editor.text.batch` changes several files in one call, all of them or none. Agents are told
  to edit through GoGarden rather than with scripts, so every change can be undone and is kept in
  Local History.
- **Places by line.** Find Usages, Show Usages, Go to Declaration and Rename take a line and a
  column, counted from 1 as everything else, instead of a character offset.
- **Short lists.** With `compact`, symbols, types, a file's structure and usages come as one line
  each (`src/store.go:12:6 struct Store`), and problems as `12:5 error undefined: x [compiler]`.
  Problems can be narrowed to errors only (`severity: "error"`), to some files (`paths`, `exclude`)
  or to a number (`limit`).
- **Usages by name.** Find Usages takes the declaration's name (`symbol: "Store.Get"`), or a line
  and the name on it, and can add the places only text knows of, such as the name in a comment or
  a string (`includeTextMatches`).
- **Problems with the edit.** `checkProblems: true` on an edit answers the errors it brought, once
  the language server has checked the new text, so no separate check is needed.
- **Part of a file.** `get_document_text` reads a range of lines, with their numbers if asked
  (`12|text`, a few bytes per line); a file larger than 32 KB is read only by ranges.
- **One way to count.** Lines and columns are counted from 1 in everything an agent reads about a
  place in a file: search matches, navigation, problems, file reads and failed tests, as the editor
  shows them, and the run targets of a file too. A search match names its file once, relative to
  the project, and so do usages (each file once, its usages under it) and the symbols and types
  that Go to Symbol and Go to Class find.
- **Clean or not checked yet.** A file's problems say whether the language server has checked the
  file as it is now: with no problems, `checked: true` means the file is clean, and `checked: false`
  says why not (not checked yet, being checked after a change, or no server).
- **Waiting for a large project.** Go to Symbol and Go to Class can wait longer than 10 seconds
  (up to two minutes) while the language server is still loading the project, as rust-analyzer
  does on a large workspace.
- **Short run targets.** What a file can run is listed as line, kind and name, without the whole
  configuration of each test (a file of 600 tests: 360 KB before, 83 KB now); a test file added
  while the IDE runs gets its run icons at once.
- **Tests without polling.** `run.process.start` with `waitMs` answers when the run has ended, with
  its state and how many tests passed and failed; `run.tests.get` with `failedOnly` then gives each
  failed test with the file and line where it failed and its message, and with `test` one test's
  output. The whole test tree, which can be hundreds of kilobytes, is never needed. A
  configuration given as such is checked first: an option its kind does not have (a misspelt
  `filter` of `cargo.test`) is refused with the kind's options, instead of running every test.
- **Each answer once.** An answer is its result as compact JSON, sent once (not also a second time
  in another form).
- **In English.** What agents get (errors, the reasons for empty answers, messages) is in English,
  whatever language your system and GoGarden's interface speak; agents work best in it. **Settings
  → Tools → MCP Server → Language for agents** chooses another. Error codes such as `not_unique`
  are the same in every language, and the interface keeps its own language.
- **No empty answers without a reason.** A search that finds nothing says so (`noMatch`), and one
  that is still waiting says why: the project is being listed, the language server is starting or
  still loading the project (with its progress). When the language server a file needs is not
  installed or does not run (rust-analyzer, gopls...), problems, usages, symbols, the structure and
  rename say so (`serverMissing`, `serverNotRunning`, `starting`) and name the server with the
  command that installs it, instead of answering nothing; a symbol search that found nothing says
  so too when a language of the project has no server, but only for a server that finds
  declarations: never for Dockerfile, YAML, TOML or JSON, and never beside what was found. Folders and files can be given relative to the
  project or as full paths; a folder that is not in the project is an error, not "nothing found". `run_action` runs anything a shortcut, a menu item or Find
Action runs, including what opens a tool window, a popup or a dialog (Settings, Find in Files, Git
→ Push…), and says what the window shows afterwards; `press_keys` presses keys in the window, for
example Escape to close a dialog, and types text after them into what they opened, such as ⇧⌘A
and the name of an action (GoGarden's window only: your keyboard is not used). What needs a text
file in the editor, such as Go to Line or Rename, is refused with a message while a diff, an image
or no file is in front. Other tools
open and close projects, read documents, select files in the project tree, drive the Terminal,
find actions, read and change the keymap and the settings, list, choose and import colour themes,
and install plug-ins and updates. Keys an agent presses count as the agent: what it may not do by a command, it may not do by a key.

Agents get their answers while GoGarden's window is minimised, hidden or covered by other
windows, or while the display sleeps; the window stays where it is. The language server's problems,
Git's status and runs keep up meanwhile. Only `press_keys` waits until the window is drawn again.

## Files agents never read

**Settings → Tools → MCP Server → Files hidden from agents** lists the files whose text agents
never get through GoGarden: one pattern per line, a path relative to the project, a file name
(`*.pem`) or a folder, and `!` before a pattern to keep the files it names readable. By default it
hides environment files (`.env`, `.env.local`; not `.env.example`), keys and certificates, SSH keys,
`.netrc` and credential files. An agent that asks for such a file gets an error instead of its
text, its searches never look into such files (they only say how many files are hidden, so a
search cannot guess a secret character by character), the commands that work on the file in
front refuse while a hidden file is in front, typing `cat .env` into the terminal is
refused (whatever the case of the name, as `.ENV` opens `.env` on a Mac, and through a link to the
file or its folder), and GoGarden shows a notification naming the file each time. Agents do not change
such files either: deleting, renaming, rolling back or creating one, or a folder that holds one, is
refused, and a stash, Replace in Files or a commit of a changelist leaves them as they are (the
answer says how many it left out); patches an agent makes never contain them, nor do the HTTP requests it runs (a request whose body
reads one, `< .env`, is refused) or the copies it makes. When an agent discards all changes, only
tracked files are reverted: it never deletes untracked files or links unless it names them. GoGarden's own data
folder (Git's undo history, shelves, Local History, settings) is closed to agents too, but for the
scratch files. Only you can change the list; an agent cannot. A program an agent runs can still read any file, so keep agents you do not
trust away from the terminal.

An agent never trusts a project. A project you have not trusted is in [safe
mode](05-project.md#trust-and-safe-mode) for agents too: no language server starts, Git is off and
nothing runs, and a headless IDE says so when it starts. Trust the project, or a folder above it, once in a window
that uses the same data folder.

`mcp.session.stats` tells how many calls an agent made, how many bytes they took and how long they
ran, per tool; each answer also says how long it took (`_meta.elapsedMs`).

## Without a window

An agent can keep GoGarden open for a whole session without any window at all:

```sh
gogarden --headless ~/src/myproject
```

GoGarden opens the project as usual: gopls, the indexes, Git, run configurations and tests,
inspections and every command work, but nothing is drawn and no window, Dock icon or GPU is used.
The MCP server is always on; it prints its address as one line of JSON (`{"url": ..., "pid": ...}`)
and picks a free port unless you give one with `--mcp-port 55800` (or `ZEN_AUTOMATION=55800`).
The agent then asks it for what a shell lacks: problems after an edit, usages, rename, test results,
Git state.

- Each project keeps a window that is not drawn: `run_action` and the `view.*` actions open and
  close its tool windows (Maven, Gradle, Problems, Run…), popups and dialogs, `get_window_state`
  says what it shows, and the tree's selection works, as in a window. Only `press_keys` answers
  that it is not available without a window: keys need a drawn frame; use the key's action instead.
- `exit_ide` (or `view.ide.exit`) ends it: runs and tasks are stopped and changed files saved.
  With `--exit-with-parent` it also ends by itself when the program that started it ends.
- It never takes what you open with `gogarden <file>` while you work: that goes to your windowed
  GoGarden. Started with `--accept-open` it takes it, as a window does.
- One project is open in one headless GoGarden at a time: a second start on it is refused and
  names the running one's address.
- It leaves your windowed GoGarden's recent projects, window layout and clipboard alone. With
  `--no-restore` (also `gogarden mcp --headless --no-restore .`) it opens the project without the
  tabs and tool windows it had last time.
- When it starts it writes to its standard error which language servers the project needs that it
  found and which are missing, with how to install them.

### Started with the agent's session

`gogarden mcp --headless [--no-restore] [<project>]` (the current folder when none is given) speaks MCP over
standard input and output, as agents that start MCP servers as programs expect. It uses the
GoGarden without a window that already has the project open, or starts one, and that GoGarden ends
by itself about a second after the last such session ends (its input closes or the agent quits),
however short the session was. Your windowed
GoGarden is never used or touched. **Settings → Tools → MCP Server** shows the command with the
right path and a Copy button.

`gogarden --version` prints the version and `gogarden --help` the ways to start GoGarden; neither
opens a window. An unknown option (`gogarden --bogus-flag`) starts nothing: GoGarden prints the
usage and exits with code 2; to open a folder whose name starts with `-`, put `--` before it. Their
messages are in the language chosen in GoGarden's settings, or the system's. `zen --version` (the
`zen-cli` inside the IDE) prints the IDE's version.

### Managing them yourself

`zen-cli ide` starts, lists and stops them and calls their tools:

```sh
zen-cli ide start ~/src/myproject      # or reuses the one that has it open
zen-cli ide list
zen-cli ide call dispatch_command --args '{"command": "problems.project.list"}'
zen-cli ide stop
```
