# Power Save Mode, the Memory Indicator and the Activity Monitor

Power Save Mode lowers the IDE's background work on battery; the Memory Indicator and the Activity
Monitor show what the IDE uses.

## Power Save Mode

**File → Power Save Mode** turns it on and off; the item has a check mark while it is on, a battery
icon shows in the top right corner of every editor, and a notification says "Power saving is on" with two links: **Do not show again** (no more balloons for it; Settings → Appearance &
Behavior → Notifications turns them back on) and **Disable power save mode**. While it is on, the
IDE pauses:

- checking files as you type: the language servers' problems and the inspections, so the editor
  shows no highlighting of errors and warnings, and the Problems tool window lists none;
- the completion popup that opens by itself while you type; **⌃Space** still opens it;
- go.mod's network checks, which look for newer versions of dependencies;
- reading changed files into the index; Find in Files still finds their new text.

gopls checks files only when they are saved. Turning Power Save Mode off, from the menu or the
notification, checks every open file again: the highlighting comes back in all open
editors without reopening them. The IDE remembers Power Save Mode across restarts; each IDE has its
own.

## Status bar widgets

Right-click the status bar, or open **View → Appearance → Status Bar Widgets**, to choose its widgets: **Line:Column Number**, **Language Services**,
**Line Separator**, **File Encoding**, **Power Save Mode**, **Indentation**, **MCP Server**,
**Read-Only Attribute** and **Memory Indicator**. A check mark shows the widgets that are on; a
click turns one on or off, and the IDE remembers the choice. Every widget but Power Save Mode and
the Memory Indicator is on at first; with the Power Save Mode widget on, the status bar shows a
battery icon while the mode is on, and a click on it turns the mode off.

## Memory Indicator

The Memory Indicator shows the memory the IDE's process takes (its resident memory), in megabytes,
measured again every few seconds. Turn it on from the status bar's menu; a click on it opens the
Activity Monitor. Zen IDE is not a Java program, so there is no heap to collect and no memory
setting to change.

## Activity Monitor

**Help → Diagnostic Tools → Activity Monitor…** lists the processor use of the IDE by part, the
busiest first, under the heading `%CPU  Subsystem`, measured every two seconds: the whole IDE
process (`<Process total CPU usage>`), the index, each language server with the processes it
started, Git, and the other programs the IDE started (runs, terminals, tools). Processor use is in
percent of one core over the last two seconds. **OK** closes it.

## The IDE log

Every IDE writes a log on your computer; nothing is sent anywhere. On macOS it is
`~/Library/Logs/<app id>/zen.log`, on Windows under `%LOCALAPPDATA%`, on Linux under
`~/.local/state`. At 5 MiB the file becomes `zen.1.log`, and the last 12 such files are kept. It
holds the start of each run, failed commands, warnings, and a crash's message with its backtrace.

- **Help → Show Log in Finder** (**Show Log in Explorer** on Windows) opens the folder with the
  current log selected.
- **Help → Collect Logs and Diagnostic Data…** says what the file will hold and, once you agree,
  writes a ZIP in the log folder and shows it: the IDE's
  logs, the language servers' logs, this window's run output, the versions, and the settings with
  passwords, tokens and keys hidden, and URLs without their user name and password. Nothing is
  uploaded; a program may have printed a secret in its console, so read the file before you attach
  it to a bug report.
- **Help → Diagnostic Tools → Debug Log Settings…** takes categories, one per line: a module path
  such as `zen_lsp::client` writes debug lines, `zen_lsp::client:trace` writes everything. Each IDE
  keeps its own list, and it applies at once.
- **Help → Diagnostic Tools → Special Files and Folders…** lists where the IDE keeps its settings,
  data, plug-ins, caches and logs; a click on a path shows it in the file manager.

## Repair IDE

When the IDE does not agree with your project (a file is not found, a highlight stays wrong),
**File → Repair IDE** puts it right one step at a time, without a restart. It refreshes the file
system: open files without unsaved changes are read from the disk again, and the Project tree is
read again. A notification says what the step did and offers:

- **Everything Works Now**, which ends the repair and closes the notification;
- **Continue**, which runs the next step;
- **More**, which opens [Invalidate Caches](#invalidate-caches).

The steps, in order:

1. **Refresh file system**.
2. **Rescan Project Indexes**: every file of the project is read into the index again, in the
   background; search keeps working.
3. **Reopen Project and Re-sync**: the language servers restart, the run configurations and the Git
   state are read again. The project and its tabs stay open.
4. **Drop Indexes and Reindex Current Project**: the index is thrown away and built from nothing.

After the last step only **Everything Works Now** and **More** are left. **Repair IDE on File** in
the Project tool window's menu of a file does the first two steps for that file alone.

## Invalidate Caches

**File → Invalidate Caches…** drops the data the IDE derived from your projects, for all projects
of this IDE, and restarts:

- **Invalidate and Restart** saves your files (a save that fails stops the restart) and restarts.
  The start removes the language servers' caches and the libraries read for them; the index, the Git
  log and its graph live in memory and are built again by any start, so the projects index again.
- **Clear file system cache and Local History** also removes [Local History](18-local-history.md):
  every saved version of every file is lost. It is off at first.
- **Clear VCS Log caches and indexes** also removes the Git log's avatar pictures. It is on at first.
- **Just Restart** restarts without removing anything, **Cancel** changes nothing.

When something is running, the IDE lists it and asks first, as on Exit. Your files, settings, run
configurations and the structure of database connections stay.

**Restart IDE…** is in Find Action (⇧⌘A) and in Settings → Keymap, without a shortcut; it asks first
unless Settings → Appearance & Behavior → System Settings has **Confirm before exiting the IDE** off.

## Open-source licences

The IDE contains open-source software. **Help → Open-source Licences** (on macOS also in the
application menu, under **About**, whose panel points to it) shows its licences: Zen IDE's own, the
notices of every library the IDE, zen-cli and the bundled plug-ins are built from, the Rust standard
library's notices and the licences of the fonts. Choose a file on the left to read it. The files are
in `Contents/Resources/Licenses/` of the application (`Licenses/` next to `bin/` on Linux and
Windows); a plug-in keeps its own notices inside its module, and a database driver program prints
them with `--licences`.

## For agents

Agents switch Power Save Mode with the window action `view.powerSaveMode.toggle` or
`set_settings {powerSaveMode}`, and read what it pauses with `powerSave.status.get`. The status bar
widgets are `view.statusBarWidgets.open {widget, shown}` (the menu's items are `view.statusBarWidgets.open/<widget>`) and the setting `statusBarWidgets`; the
Memory Indicator is `diagnostics.memory.get` and the Activity Monitor `diagnostics.activity.get`
(the dialog: `view.activityMonitor.open`). The log is `diagnostics.log.show`,
`diagnostics.logs.collect { path?, noReveal?, wait? }`, `diagnostics.logCategories.get`,
`diagnostics.logCategories.set { categories }` and `diagnostics.folders.list`; their dialogs are
`view.collectLogs.open`, `view.debugLogSettings.open` and `view.specialFiles.open`. The licences are
`view.openSourceLicences.open`, which answers the folder and its files. Details are in `docs/automation.md`.

(the dialog: `view.activityMonitor.open`). Repair IDE is `ide.repair.run { step? }`,
`ide.repair.finish` and `ide.repair.file { path }`; Invalidate Caches is
`ide.caches.invalidate { clearVcsLog }` (only a person clears Local History) and then `view.ide.restart`, which asks the person, and
`view.invalidateCaches.open` opens the dialog. Details are in `docs/automation.md`.
