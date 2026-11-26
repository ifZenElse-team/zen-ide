# Accessibility

Zen IDEs and Zen Toolbox can be used with a screen reader such as VoiceOver on macOS: every button,
tab, list and tree row and field has a role and a name, the editor reads as a multi-line text field
with its caret, and notifications are announced. This support is new and has not yet been checked
with every screen reader, so some parts may still be hard to use (see "What is not read yet").

## Turning it on

There is nothing to turn on in the IDE. Start your screen reader (VoiceOver: ⌘F5, or System
Settings → Accessibility → VoiceOver) and the IDE gives it what it shows. The IDE builds this
description only while a screen reader asks for it, so without one it costs nothing. **Customize**
on the welcome window shows **Support screen readers** on, and it cannot be switched off, for this
reason.

## What a screen reader reads

- **The window**: the main menu bar, the toolbar (Back, Forward, the project and branch widgets,
  the run widget with the selected configuration, Run, Debug, Stop, Settings, Search), the tool
  window stripes (each button says whether its tool window is open) and the status bar (the file's
  path, the caret position, which opens Go to Line, the line separator, the encoding, the indent, the read-only lock,
  the language server widget and the MCP indicator).
- **Tabs**: editor tabs (the selected one, "modified" for unsaved files, and their Close buttons),
  project tabs, terminal tabs and the tabs of the Problems, Find, Run, Debug and Git tool windows.
- **Trees and lists**: the Project tree (each row's depth, whether a folder is expanded and which
  rows are selected), Structure, Find Usages, the Refactoring Preview, Problems, the test tree of the
  Run tool window, the threads, frames and variables of the Debug tool window, Search Everywhere,
  Recent Files, Paste from History, Find in Files, completion, context actions and the Git log.
- **Titles and text**: tool window titles are headings; the Run console reads as a log whose new
  output is announced (its last lines, at most once a second); notifications and the editor's banners (a file changed on disk, a missing
  language server) are announced when they appear.
- **The editor**: a multi-line text field named after the file, with the lines around the view and
  around the caret, the caret and the selection. Moving the caret or selecting text with the
  keyboard is followed by the screen reader. The gutter's run icons, the context actions bulb,
  folding arrows, change markers, problem marks and Code Vision are buttons and links with names.
  With the editor split, each editor's name adds its group ("Editor: main.go, group 2 of 2") and
  the dividers between the groups are named splitters.
- **Dialogs**: Settings (the section tree, every field and its label, OK, Cancel and Apply), Go to
  Line, Rename, Run/Debug Configurations and the welcome screen.

## What is not read yet

- The Git workspace tab and the branches and changes panels of the Git tool window.
- The terminal's contents: the terminal is named, but its screen is not read.
- Moving the keyboard between tool windows (as ⌘1 and Escape do) is not announced, and a screen
  reader cannot yet place the caret or select text in the editor by itself.
- Linux (Orca) and Windows (Narrator) have not been tried.

## Trying it with VoiceOver

A short check of the main paths, with VoiceOver on (⌘F5) and its cursor moved with ⌃⌥ and the
arrow keys:

1. Open a project. VoiceOver reads the window's title and, moving through it, the toolbar buttons
   by name ("Back", "Settings", the run configuration).
2. Move to the left stripe: each button reads its tool window and whether it is pressed; ⌃⌥Space
   on "Project" closes and opens the Project tool window.
3. In the Project tree, rows read their names, "expanded" or "collapsed" for folders and their
   level; open a file with ⌃⌥Space twice.
4. In the editor, VoiceOver says "Editor: main.go, edit text". Move the caret with the arrow keys:
   the characters and lines are read; select with ⇧→ and hear the selection.
5. The editor tabs read their names, "selected" for the active one and "modified" after an edit.
6. Open Problems (⌘6): its title is a heading and each problem reads its severity, message and line.
7. Press ⇧⇧: the Search Everywhere field reads its hint and the results their names.
8. Open Settings (⌘,): the section tree, the fields with their labels and OK, Cancel and Apply.
9. Run a configuration: the Run tab and console are read, and new output is announced.
10. Trigger a notification (for example a command that cannot run): it is announced.
