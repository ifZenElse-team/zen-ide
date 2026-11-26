# ADR-0158: Power Save Mode, status bar widgets and the Activity Monitor

- **Status:** Proposed
- **Date:** 2026-10-25 (checked again on 2026-10-26)

## Context

The requirement: File → Power Save Mode, which turns off inspections, on-the-fly
highlighting and the completion popup and is kept across restarts; a status bar context menu of
widgets (Status Bar Widgets), among them Power Save Mode and the Memory Indicator, both off by
default; and Help → Diagnostic Tools → Activity Monitor…, a plain monospaced
text of the processor use by subsystem (`%CPU  Subsystem`) with an OK button. While Power Save Mode
is on, the editor's inspection widget shows a battery icon and a balloon "Power save mode is On"
offers Do not show again and Disable power save mode; the mode is kept IDE-wide. Markers must not
stay missing after Power Save Mode is turned off. Zen IDE has no JVM, and its background work is the
language servers, the inspections (go.mod's are network checks), the file and text index and Git.

## Decision

1. **Power Save Mode** is the IDE setting `powerSaveMode`, kept per IDE like every IDE setting,
   and the session setting `ide.powerSaveMode` written from it. While it is on:
   - the language servers' diagnostics and the inspections give no problems
     (`problems.file.list` answers `reason: "powerSave"`), pulled diagnostics are not asked for,
     and inspectors are not run (go.mod's network checks with them);
   - a server that declares `powerSave` in its declaration (`language-servers.json`) gets those
     settings laid over its own, object by object, at start and through
     `workspace/didChangeConfiguration` (gopls: `"diagnosticsTrigger": "Save"`); they are taken off
     again when it is turned off;
   - typing opens no completion popup by itself; ⌃Space still does;
   - the index keeps the file names current but defers reading the text of changed files
     (`FileIndex::set_paused`); Find in Files reads those files in full meanwhile, and they are read
     when the mode is turned off.

   Turning it on shows a battery icon in each editor's top right corner, where the inspection
   widget's counts are, and posts a warning notification "Power save mode is On" in a group of its
   own (`powerSave`) with the actions Do not show again (the window action
   `view.notifications.balloonsOff { group }`, which adds the group to `balloonGroupsOff`) and
   Disable power save mode (`view.powerSaveMode.toggle { on: false }`).
2. **Turning it off checks every open file again**, so that highlighting comes back without
   reopening editors: pulled diagnostics are refreshed, and a server
   that only publishes gets each open file reopened (`didClose`/`didOpen`).
3. **Status bar widgets** are a right-click menu of the status bar with check marks: Line:Column
   Number, Language Services, Line Separator, File Encoding, Power Save Mode, Indentation, MCP
   Server, Read-Only Attribute and Memory Indicator, in
   this order. The setting `statusBarWidgets` keeps only the widgets that differ from the default:
   all shown but Power Save Mode and the Memory Indicator. MCP Server is on
   by default, because ADR-0033 wants a connected agent visible in the status
   bar. The Power Save Mode widget, when turned on, shows a battery while the mode is on, and a
   click turns the mode off.
4. **The Memory Indicator** shows the resident memory of the IDE's process, measured every 3 s on a
   thread of its own (`ps` on macOS, `/proc` on Linux); with no JVM there is no "used of max" and
   no garbage collection, so a click opens the Activity Monitor instead.
5. **The Activity Monitor** is a dialog: a bordered monospaced text, `%CPU  Subsystem`,
   an empty line and a row per part in angle brackets (`<Process total CPU usage>` for the IDE
   process), the busiest first, and OK. It measures every 2 s, while asked within the last 10 s, the IDE process,
   the index (its worker threads' busy time and an estimate of its tables), each language server's
   process tree, the Git processes the IDE started, and its other child processes by program;
   processor use is in percent of one core over the last interval. Memory per part is measured
   too but only `diagnostics.activity.get` gives it; the dialog shows no memory.
6. Every capability is reachable by agents (ADR-0033): the window actions
   `view.powerSaveMode.toggle`, `view.notifications.balloonsOff`, `view.statusBarWidgets.open`,
   `view.activityMonitor.open`, the
   commands `powerSave.status.get`, `diagnostics.memory.get`, `diagnostics.activity.get`, and the
   two settings.

## Consequences

- A language server that needs Power Save settings declares them in data; the platform knows no
  language.
- Change Memory Settings, Dump Threads and the profilers of Diagnostic Tools stay out: they are JVM
  tools without a counterpart here.
- The Activity Monitor's figures are per process and, for the index, estimates; there is no
  per-plug-in breakdown while plug-ins run inside the IDE's process.
