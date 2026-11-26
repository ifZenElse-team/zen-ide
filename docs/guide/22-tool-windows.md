# Tool windows

Tool windows (Project, Commit, Structure, Bookmarks, Terminal, Problems, Run, Debug, Find, Git,
Services, TODO, Hierarchy, Caffeine, Maven, Gradle, Notifications) sit around the
editor. You choose where each one is and how big it is, and every project
remembers its own layout.

## The stripes

The narrow bars at the left and right edges of the window hold a button for each tool window. A
click shows the tool window, another click hides it; the button is lit while it is shown. Each
stripe has three groups:

- **Left top** and **left bottom** (after a thin line) on the left stripe, **right top** and
  **right bottom** on the right stripe: tool windows shown at that side.
- The group at the bottom of each stripe: tool windows shown at the bottom of the window, on its
  left half (**bottom left**) or its right half (**bottom right**).

By default Project, Commit and Structure are at the left top, Bookmarks (**⌘2**,
[Navigation](navigation.html#the-bookmarks-tool-window)) at the left bottom, Find, Debug, Run,
Terminal, Problems, Git, Services and TODO at the bottom left, Hierarchy and
[Caffeine](24-caffeine.md) at the right top (Caffeine under the Git button; its coffee cup shows
whether the computer is kept awake), then [Maven and Gradle](26-maven-and-gradle.md) at the right
top too, listed only in a project their plug-in supports, and Notifications at the right bottom
(its bell has a dot while something is unread).

One tool window shows in a group at a time: opening Structure hides Project when both are at the
left top. Two groups of the same edge share it when both show a tool window: left top above left
bottom, bottom left beside bottom right, right top above right bottom. Drag the gap between them to
give one more room.

## Moving a tool window

- **Drag its stripe button** to another place on either stripe. A line shows where it goes: among
  the buttons of a group, under the last top button for the bottom group of a side, or among the
  buttons at the bottom of a stripe. A shown tool window shows in its new place.
- **Right-click its stripe button** and choose **Move to** → **Left Top**, **Left Bottom**,
  **Bottom Left**, **Bottom Right**, **Right Bottom** or **Right Top**; the side it is at is
  greyed out. The menu also has **Hide**.
- **⋮** in the header of the Project, Structure, Bookmarks, TODO and Terminal tool windows has the
  same **Move to**.

## View modes

Each tool window has a **View Mode**: in **⋮** of its header (Project, Structure, Bookmarks, TODO and
Terminal) and in **Window → Active Tool Window → View Mode** for the active one. The current mode
has a check.

- **Dock Pinned** (the default): at its side next to the editor, shown until you hide it.
- **Dock Unpinned**: at its side, but it hides as soon as you click somewhere else (the editor,
  another tool window) or open another tool window. A click on a stripe leaves it shown.
- **Undock**: over the editor at its side, without taking the editor's room. It stays while you
  work in the editor and hides when another tool window opens.
- **Float**: in a small window of its own that stays above the IDE while the IDE is in front.
- **Window**: in an ordinary window of its own, which other windows can cover.

Float and Window can be moved anywhere, to another screen too, and open there again. Closing such a
window hides the tool window; its stripe button shows it again in the same mode.

## Layouts

**Window → Layouts** lists **Default**, then each layout you saved, with a check on the current one,
then **Save Current Layout as New…**.

- **Save Current Layout as New…** asks for a name and saves where every tool window is, its size,
  whether it is shown and its view mode, and makes it the current layout. Saved layouts belong to
  the IDE: every project offers them.
- **Default** puts every tool window back at its default place and size, docked and pinned, with
  only the Project tool window shown.
- Each saved layout is a submenu:
  - **Restore** applies it to the project in front and makes it current.
  - **Save Changes** saves the tool windows as they are now into it.
  - **Rename** asks for a new name.
  - **Delete…** asks before deleting it; the current layout cannot be deleted.
- **⇧F12** restores the current layout: the one you saved or chose last, or Default.

## Resizing

- Drag the edge between a tool window and the editor. The tool windows of one edge share their
  width (or height at the bottom), so switching between them keeps it.
- In a window too small for them, the tool windows shrink so that the editor keeps some room; they
  get their own size back when the window is larger again.
- **Stretch to Left** (**⌃⌥←**), **Stretch to Right** (**⌃⌥→**), **Stretch to Top** (**⌃⌥↑**) and
  **Stretch to Bottom** (**⌃⌥↓**) move the edge of the active tool window, the one you opened or
  clicked in last; along its edge they move the line between two tool windows. They are also in
  **Resize** of **⋮** and of **Window → Active Tool Window**. In the editor, **⌃⌥↑** and **⌃⌥↓**
  go to the previous and next highlighted usage instead.

## Hiding and the default layout

- **⇧⎋** (Hide Active Tool Window, **Window → Active Tool Window**) hides the active tool window,
  from the editor or from the tool window itself. While a popup, a menu or a dialog is open, or a
  text field or the Terminal has the keyboard, ⇧⎋ goes to them instead. **—** in a tool window's
  header hides that one.
- **⇧⌘F12** (Hide All Tool Windows, **Window → Active Tool Window**) hides every tool window shown;
  pressed again with none shown, it shows them again.
- **Window → Layouts → Default** puts every tool window back at its default place, order and size,
  with only the Project tool window shown; **⇧F12** restores the current layout
  ([Layouts](#layouts)).

## Background tasks

Long work runs in the background while you go on working: a dependency update (`go get`), a Git
fetch, pull or push. The status bar shows the running task, its title and a progress bar, to the
left of the file's widgets (line and column, encoding, indentation), so a task that starts or ends
never moves a widget under your pointer; a task that just ended shows how it went for a moment, a failed one until you look at
it. Click it to show or hide the **background tasks** panel above it: every task of the window, and
the live log of the chosen one, what its program prints as it prints it, then whether it finished
or the error. A dependency update opens the panel at once; **Hide** hides it, and the task goes on.
**Window → Processes** shows and hides the panel too, also when nothing is running.

A task that can be stopped has **×** next to its progress, in the status bar and in the panel: a
Git fetch or push, a dependency update, a Maven or Gradle reload, a file watcher, a search, and a
language server's work when the server allows it. Its programs are asked to stop and killed 2
seconds later if they have not; files a cancelled dependency update changed are put back. The task
then shows as cancelled. Downloads of JDKs and build tools cannot be cancelled yet: they end by
themselves and leave nothing half-written.

**Indexing…** shows how many files of how many are done. It cannot be cancelled, but **‖** pauses
it and **▷** resumes it. While it is paused, what is indexed so far stays usable: Find in Files reads
the files not indexed yet, and Go to Class and Go to Symbol say they are not available while a
language server is indexing.

A language server's own work, such as loading the workspace, shows as a task named after the server,
with the server's percentage and message; it disappears from the list when it ends.

AI agents list the tasks with `tasks.background.list`, read a log with `tasks.log.read`, stop one with
`tasks.background.cancel`, pause and resume one with `tasks.background.pause` and
`tasks.background.resume`, and show or hide the panel with `tasks.panel.show`, `tasks.panel.hide` and
`tasks.panel.toggle` ([Automation](../automation.md)).

## Inspecting code

**Code → Inspect Code…** checks more than the open files. Choose the scope: **Whole project**, a
**Directory**, the **Selected files** (of the Project tool window, or the active file), the
**Uncommitted files** or a **Custom scope**, and whether **Include test sources** counts.
**Inspection profile** chooses the [profile](09-settings.md#editor--inspections) for this run.
**Configure…** opens **Settings → Editor → Inspections**. The dialog remembers your choice for the
project. **Analyze** starts a background task, which you can cancel in the status bar; a cancelled
run shows nothing.

The inspectors look at every file in scope, and language servers get each file opened for a moment,
a few at a time. Spelling covers open files only. A file a server did not report on is counted above
the results; agents read why. With Power Save Mode on, language servers are not asked.

The results open in the **Problems** tool window, in a tab **Inspection Results: <scope>** with a ×
to close it. The tree goes severity, inspection, file, problem:

- **Group by Directory** adds the folders under each inspection.
- **Enter**, **F4** or a double click jumps to the source. The arrows move and fold.
- On the right are what the inspection reports, the problem, and its quick-fixes as buttons; on a
  file, **Fix All in File**, and on any other group, **Fix All**. **Suppress** is among those
  buttons ([below](#suppressing-a-problem)); what it covers leaves the results.
- **Exclude** (or **Delete**) leaves a problem or a group out of the tree and of exports. **Show
  Excluded** lists them again, greyed; **Exclude** on one of those takes it back.
- **Rerun Inspection** runs the same check again. Places are those of the run: rerun after editing.
- **Export to HTML…** and **Export to XML…** write the results to a file.

### Suppressing a problem

When a problem is right in general but not here, say so in the source: **⌥⏎** on the problem, or
the context menu of its row in the **Problems** tool window, or the buttons of Inspection Results,
offer **Suppress … for statement**, **for function** and **for file**. The IDE writes the language's
own mechanism, which your other tools see too:

| Language | Statement | Function | File |
|---|---|---|---|
| Go | `//nolint:<id>` at the end of the line | `//nolint:<id>` above `func` | `//nolint:<id>` above `package` |
| Rust | not offered | `#[allow(<id>)]` above `fn` | `#![allow(<id>)]` on the first line |

A second inspection joins the directive that is there. It is an ordinary edit, so **⌘Z** takes it
back. The IDE honours `//nolint`, `//nolint:all` and `//nolint:a,b` you write yourself, in the editor,
the Problems tool window and Inspect Code. An error from the compiler cannot be suppressed.

**Code → Analyze Code → Run Inspection by Name…** (⌥⇧⌘I, Ctrl+Alt+Shift+I) picks one inspection or
gopls analyzer by name, asks for the scope and **File mask(s)** such as `*.go,!*_test.go`, and shows
the same results.

**Code → Code Cleanup…** asks for the same scope and a profile, then applies the fixes of the
inspections marked as cleanup ones (**Cleanup only** on the
[settings page](09-settings.md#editor--inspections)) that the profile leaves on: in Go, the gopls
analyzers that simplify code, such as `simplifyrange` and `simplifyslice`. It runs in the background,
one fix at a time, and opens no results; a notification says how much it fixed. **Code → Analyze
Code → Silent Code Cleanup** does the same without asking, for the files selected in the Project
tool window or the current file.

Open files keep their fixes unsaved, and one **⌘Z** takes back all of a file's. Other files are
written at once; the **Undo** of the notification takes back the whole cleanup, open files too.

AI agents run `inspect.code.run`, follow it with `inspect.code.status`, read
`inspect.results.tree` and `inspect.results.list`, and use `inspect.results.fix`,
`inspect.results.exclude`, `inspect.results.rerun` and `inspect.results.export`
([docs/automation.md](../automation.md#inspect-code)).

In a project that is not trusted, an inspection or a fix that runs a program of the project does not run:
the fix says why. An agent sees nothing of the files hidden from it in results, even a run you started; it
cannot fix or suppress there, and it exports results and inspection profiles only inside the project.

## TODO

The **TODO** tool window (**View → Tool Windows → TODO**, or its stripe button; it has no shortcut,
⌘6 is Problems) lists every TODO and FIXME comment: by default, a line of a comment with
the word `todo` or `fixme`, in any case (`// TODO: …`, `/* fixme … */`, `# Todo …`). Only comments
count, judged by each file's language: the word in a string or in code is not an item, and a file
whose language the IDE does not highlight has none. Excluded folders (and `target`, `vendor`,
`node_modules`) are left out.

- **Tabs.** **Project**: every file of the project. **Current File**: the file in front in the
  editor. **Scope Based**: the files of the scope chosen in its **Scope** list, the same scopes as
  Find in Files, the project's own scopes from `.idea/scopes` among them. **Current Changelist**
  (only for a project in Git): the files of the active changelist.
- **The tree** has a line "Found N TODO items in M files", then each file with its folder and its
  count, and under it each item with its line number and the line's text, the TODO part coloured.
  A click selects an item; a double click, **⏎** or **F4** opens it in the editor. With **Navigate
  with Single Click** a click, or moving the selection with the arrows, opens it at once.
  **←** and **→** fold and unfold a file, **⎋** goes back to the editor.
- **The toolbar** on the left: **Previous Occurrence** (**⌥⌘↑**) and **Next Occurrence** (**⌥⌘↓**)
  select the item before or after the selected one and open it, unfolding its file, from anywhere
  in the window while the TODO tool window is shown; **Navigate with Single Click**; **Expand All**
  (**⌘+**) and **Collapse All** (**⌘−**); **Preview Source** shows the selected item's file beside
  the tree, scrolled to its line. The last two toggles are in **⋮** too.
- **It follows your work**: a moment after you type, after a file changes on disk, after another
  file comes to the front or the changelist changes, the items are read again in the background,
  only from the files that changed; unsaved changes count.

**Settings → Editor → TODO** holds what counts as an item:

- **Patterns**: regular expressions, `\btodo\b.*` and `\bfixme\b.*` by default, with **+**, **−** and
  edit. A pattern has **Case Sensitive**, an **Icon**, and either the colour scheme's TODO colour or
  a colour of its own. The tool window, the completion in comments and the highlighting all use
  these patterns.
- **Filters**: named sets of patterns, also with **+**, **−** and edit.
- **Continue a TODO on the indented lines below it** (on by default): comment
  lines below an item that are indented further than its text join the item, in the tool window and
  in the highlighting.

**Filter TODO Items** on the toolbar shows one filter's items; **Edit Filters…** opens the settings.
In the editor, TODO text takes the colour scheme's `todo` colour (or the pattern's own) and has a
mark at the right edge, along the scroll bar.

AI agents list the items with `todo.items.list` (`filter` names a filter), read the marked text with
`todo.document.ranges`, change the patterns and filters with `set_settings` and `todo`, and drive the
window with `view.todo.*` ([AI agents](11-ai-agents.md)).

## What is remembered

For each project, GoGarden keeps where each tool window is, its order on its stripe, its size,
whether it is shown, its view mode and where its floating window is, and brings them back when the project opens, and the TODO tool window's tab,
scope, Preview Source and Navigate with Single Click. Find, Run, Debug and Hierarchy start hidden,
since they show results; Hierarchy also keeps its scope, Sort Alphabetically and Navigate with
Single Click. A project last opened in an older version opens with its Project or
Structure tool window and its Terminal as they were.

AI agents move tool windows with `view.toolWindow.move`, change their view mode with
`view.toolWindow.viewMode`, manage layouts with `view.layout.saveAsNew`, `view.layout.apply`,
`view.layout.restore`, `view.layout.saveChanges`, `view.layout.rename` and `view.layout.delete`,
and read the layout with `get_window_state`
([AI agents](11-ai-agents.md)).
