# Terminal

**⌥F12** opens the Terminal tool window at the bottom: your login shell, in the project's folder.

Switching to the terminal saves every modified file first ([Autosave](02-editor.md#autosave)), so
what you run there sees what you typed.

## Tabs

- **+** or **⌘T** (while the terminal has the keyboard) opens another tab in the project's folder;
  **×** on a tab closes it with everything running in it.
- Tabs are named **Local**, **Local (2)**, **Local (3)**…; while your shell sets a short title
  (the command running, for example), the tab shows that instead. A long title, such as the usual
  `user@host:~/path`, leaves the tab its short name.
  **Double-click** a tab, or choose **Rename Tab…** in its menu, to give it a name of your own;
  Enter keeps it, Esc cancels, and an empty name brings the short name back.
- A long name is cut with "…"; hold the pointer over the tab to read all of it.
- When the tabs do not fit in the header, it shows as many whole tabs as fit, the tab shown always
  among them. Scroll with the mouse wheel or swipe sideways over the tabs to see the others, or pick
  any tab from the list that **⌄** next to **+** opens. **+**, **⌄**, **⋮** and **−** are always
  there, however narrow the window.

## Split panes

- **Split Right** and **Split Down** are in the menu of a pane (right-click it) and in **⋮** in the
  header. The new pane runs its own shell, in the same folder, and gets the keyboard.
- Click a pane to type in it, or move between the panes of a tab with **⌥⇥** and **⌥⇧⇥**.
- Drag the line between two panes to resize them.
- **⌘W** (or **Close Session** in the menu) ends the shell of the pane with the keyboard and removes
  the pane; typing `exit` does the same. A tab whose last pane closes closes too.

In the terminal, **⌘T** and **⌘W** act on the terminal; elsewhere they keep their usual meaning
(Update Project and closing the editor tab). Settings → Keymap lists the terminal's shortcuts.
Keys without **⌘** — **⌃C**, **⌃D**, **⌃R**, the arrows, **⇥**, the function keys — go to the
shell, even where the IDE has a shortcut on them (Run is **⌃R** elsewhere); only **⌘** shortcuts
and **⌥F12** reach the IDE while a pane has the keyboard.

## Selecting and copying

- **Drag** selects text; **double-click** selects a word, and a path (`src/main.rs:12:5`) or a web
  address is one word; **triple-click** selects the whole line, with the parts the terminal wrapped
  onto the next rows. **Shift-click** extends the selection to the place clicked, and **⌥-drag**
  selects a rectangle. Dragging above or below the pane scrolls it.
- The selection stays on its text while programs print and while you scroll the history; typing
  clears it.
- **⌘C** copies the selection: without the blanks at the ends of lines and with a line the terminal
  wrapped put back together, while real line breaks stay. With nothing selected, **⌘C** sends
  Ctrl+C to the program, as before.
- The context menu has **Copy**, **Copy as Plain Text**, **Paste** and **Select All** (⌘A).
  **⌘V** pastes as one block when the program asks for it (bracketed paste), so a shell or an
  editor does not run the lines one by one.
- The selection colour comes from the theme (`terminal.selection`, [Themes](../themes.md)). The
  terminal does not pass the mouse to programs, so selecting works in full-screen programs such as
  `vim`, `less` and `htop` too.

## Links

The Terminal finds the links in what programs print: web addresses (`https://`, `http://`, `ftp://`,
`file://` and the like) and paths of files and folders, with the line and column that compilers and
test runners print after them — `src/main.rs:12:5`, `./main.go:7:2:`, `src/app.ts(3,14)`,
`File "tool.py", line 9`. A relative path is looked for in the folder the tab's shell started in,
then in the project; `a/` and `b/` in front of a path (as `git diff` prints it) are dropped when
that finds the file. A path is a link only when it names a file or folder that exists. A link that
wraps onto the next row is still one link.

- **Hold ⌘** and point at a link: it is underlined and the pointer becomes a hand. Release ⌘ and
  the underline goes at once. Without ⌘ nothing is underlined and a click is an ordinary click in
  the terminal: it never opens anything.
- **⌘-click** a link to choose what to do with it, in a dialog over the window:
  - a web address: **Open in Browser** (the default) or **Copy Link**;
  - a file: **Open in Editor** (the default; at the printed line and column), **Reveal in Project**
    (selects it in the Project tool window), **Open in Finder** (Explorer on Windows, the file
    manager on Linux) or **Copy Path**. A folder has the same choices and opens in the Project tool
    window by default.

  **Enter** takes the highlighted default, **Esc** or **Cancel** closes the dialog.
- **Remember my choice** in the dialog makes ⌘-click take the same action for that kind of link
  (web addresses, or files) without asking. To be asked again, choose **Ask every time** in
  Settings → Tools → Terminal → Links.

## Hiding and coming back

- Hiding the Terminal (**⌥F12**, or **−** in its header) leaves the shells running: commands keep
  working and their output is there when you open it again.
- **Quitting the IDE, closing the project window or a crash leave the shells running**, with
  everything started in them — a build, a server, an AI agent in the middle of its work. They run in
  a small helper program of their own, `zen-pty-host`, that the IDE starts with its first terminal.
  When GoGarden opens the project again, the Terminal comes back with the same tabs attached to
  the same shells, and you go on where you were: each tab shows its screen and history as they are
  now, with what was printed while the IDE was away, colours included. A full-screen program
  (`vim`, `htop`, an AI agent's interface) is shown as it was and redraws itself at the pane's
  size.
- **Closing a pane or a tab ends its shell** with everything it started, as before.
- **Updating the IDE never ends the shells.** The IDE does not run the helper from its own
  installation: it copies it into its data folder (`terminal/bin/<protocol>/<build>/`, readable and
  writable only by you) and runs that copy, so Zen Toolbox can replace or remove the old version
  while the shells go on. The IDE checks the copy's contents before it starts it and makes a new one
  when they changed; copies that no helper runs any more are removed when a new one is made. The
  updated IDE attaches to the running helper when it still speaks its language (its protocol).
  When it does not, the old helper keeps its shells until they end, their tabs say that they run in
  an older terminal host this version cannot show, and they close by themselves when that helper
  stops; new tabs open in a new helper.
- The helper stops by itself a minute after its last shell ended while no IDE is open.

## Environment

**Settings → Tools → Terminal → Environment → Give programs the environment of my login shell** (on
by default). When the IDE starts, it asks your login shell (`$SHELL -l -i`, in your home folder)
for its environment, once and in the background. Terminal tabs, runs, language servers and plug-in
tool windows then get it, as if they were started from your terminal: the same `PATH`,
`KUBECONFIG`, version managers and proxies, even when the IDE was started from the Finder or Zen
Toolbox.

- What a run configuration or its `.env` files set still wins.
- The IDE keeps its own `HOME`, `USER`, `TMPDIR` and `SHELL`. It never takes the shell's
  prompts, `TERM`, the IDE's own `ZEN_*` variables or the dynamic loader's (`DYLD_*`, `LD_*`).
- If your shell fails, prints no environment or takes more than 10 seconds, programs get the IDE's
  own environment, as before, and the reason is in the IDE's log.
- Turning the setting off takes effect at once; turning it on, at the next start.
- Your shell profile runs in your home folder, never in a project's, so this happens in safe mode
  and in untrusted projects too. It changes nothing about what those may run.
- Agents see whether it is on, and the names (never the values) of the variables, with
  `ide.environment.get`.

## After a restart of the computer

A restart of the computer (or a crash of the helper) ends the shells and everything in them. The
Terminal still comes back as you left it:

- **What comes back**: every tab with its name and splits; in each pane its last screen and history
  (up to the last 2,000 lines), **dimmed**, under a line such as "Session ended: the computer
  restarted" or "Session ended: the terminal host stopped"; then a **new shell** in the same folder
  with the environment the tab was started with. The IDE keeps this in its data folder
  (`terminal/sessions/`, readable only by you) while the shells run, and removes it when a shell
  ends or the tab is closed.
- **The last command is offered, never run**: a bar over the pane shows the last command you ran in
  that shell, with **Run Again** and **Dismiss**. Nothing runs until you click Run Again, and an AI
  agent cannot do it for you. What you type while the terminal does not show it, such as a password
  the shell reads, is never taken for a command.
- **What does not come back**: the processes — the shell, a build or server that was running, an
  agent's turn in progress; anything started in the background; changes made inside the shell
  (`export`, `cd` after the start: the folder is the one the tab started in). Colours of the old
  output are not kept, only its text.
- **Turning it off**: Settings → Tools → Terminal → Sessions → **Restore terminal tabs after a
  restart**. Off, nothing of the screens is kept (what was kept is removed), and after a restart the
  tabs come back with their names and folders and a new shell only.

A folder that is gone or that you cannot open any more is replaced by the project's folder.

### AI agents continue their conversation

An AI agent that was running in a tab (Claude Code, Codex, Qwen Code or one you add) comes back in
the same conversation: after the restart the tab runs the agent's own resume command, such as
`claude --resume <id>`, in its new shell, and the line under the old output says "Resuming the
conversation of Claude Code". The turn the agent was in the middle of is not redone. An agent the
IDE has no adapter for, or that did not report its conversation, gets the offer to run its command
again, as any other command.

For this the agent has to tell the IDE its conversation id. Every terminal has `ZEN_TERMINAL_ID`
and `ZEN_TERMINAL_AGENTS` in its environment; add a **session-start hook** to the agent's own
settings that writes the hook's input there. Settings → Tools → Terminal → AI Agents (and
`terminal.agent.list` for agents) shows the command for each agent; for Claude Code it is:

```sh
[ -n "$ZEN_TERMINAL_ID" ] && [ -d "$ZEN_TERMINAL_AGENTS" ] && cat > "$ZEN_TERMINAL_AGENTS/$ZEN_TERMINAL_ID-claude-code.json"; true
```

| Agent | Where the hook goes | The event | Resumed with |
|---|---|---|---|
| Claude Code | `~/.claude/settings.json`, `hooks` | `SessionStart` | `claude --resume <id>` |
| Codex | `~/.codex/hooks.json` (or `[hooks]` in `~/.codex/config.toml`) | `SessionStart` | `codex resume <id>` |
| Qwen Code | `~/.qwen/settings.json`, `hooks` | `SessionStart` | `qwen --resume <id>` |

Use the command with the agent's id at the end (`codex`, `qwen-code`). Outside the IDE the variables
are not set and the hook does nothing. An agent connected over MCP can also report its id itself
with `terminal.agent.report`. For an agent no plug-in knows, add it in Settings → Tools → Terminal
→ AI Agents: its name, the program that runs it and its resume command with `{session}` where the
id goes. The IDE only ever runs an adapter's own resume command, and an id is only letters, digits
and `-_.:`; turning restoring off (above) turns resuming off too. The IDE resumes by itself only
with the adapters you add and those of the plug-ins that come with it; a resume command from a
plug-in you installed yourself is only offered, in the same bar as any other command, and runs when
you click Run Again. **Only you choose the adapters**: an AI agent cannot add, change or remove one
in the settings, and cannot install a plug-in that brings adapters or switch one on; it can only
report the id for an adapter that is already there.
- On Windows, and when the helper is missing from the installation, the shells still end with the
  IDE.
- Drag the edge above the Terminal to make it taller or shorter. The Terminal shares the bottom of
  the window with Problems, Run, Debug, Find and Git: one of them shows at a time. Drag its stripe
  button to move it elsewhere ([Tool windows](22-tool-windows.md)).

## AI agents

Agents connected over MCP (Settings → Tools → MCP Server, off until you turn it on) can open tabs,
split them, type into a session and read its screen with the `terminal.*` commands
([AI agents](11-ai-agents.md)). They type into a shell exactly as you would, so connect only agents
you trust. The tabs remembered from the last time are there for them even while the Terminal is
hidden: listing, reading or typing starts their shells without showing it.

`terminal.link.list` gives agents the links a session's screen shows, with their places, and
`terminal.link.open` does what ⌘-click offers: agents may open files in the editor, reveal them and
copy links and paths, but **only you open a web address in the browser** — an agent's request to do
so is refused with the code `userOnly`. The remembered choices are the `terminalLinks` setting
(`get_settings`, `set_settings`).

`terminal.selection.get` reads what is selected, as ⌘C would copy it; `terminal.selection.copy`
copies it, `terminal.selection.selectAll` selects the whole history, and `terminal.lines.copy`
copies a range of lines (from 1, the oldest). Pasting your clipboard into a terminal
(`terminal.session.paste`) is yours: an agent's request is refused with `userOnly`.
