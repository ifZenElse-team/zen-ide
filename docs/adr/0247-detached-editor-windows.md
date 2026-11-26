# ADR-0247: Detached editor windows

- **Status:** Proposed
- **Date:** 2026-11-05

## Context

The requirement: an editor tab moves into a window of its own (Open Tab in New Window, ⇧F4, in the
tab menu only). The UI specification (`docs/behaviour.md`) described the editor as having no
detached mode. Detached tool windows already run in egui deferred viewports, sharing the project's
session and view.

## Decision

1. **A window is an editor group outside the split tree.** `EditorGroups` gets `windows`, each a
   `DetachedWindow`: one `EditorGroup` with ids from `WINDOW_FIRST`, and its bounds. Every lookup,
   move and close of groups works on them as on any group, so `editor.split.moveTab`, the tab
   commands and `file.document.*` need no new paths. They are not `OwnedGroup`s: those are workspaces,
   which expand in the main window. A window holds one group: no splits inside it.
2. **One document.** A window is a view of the workbench's document, as a split is, so edits, saves
   and reloads show everywhere by construction.
3. **Each window is a deferred viewport of the project window**, as a detached tool window is. Its
   frame draws the group and runs the shortcuts; the focused group moves to it on a press.
4. **Closing.** Closing the last tab closes the window. Closing the window itself gives its tabs back
   to the main window, which can lose nothing; whether they should close instead is still open.
5. **Dragging.** A tab released outside the window opens in a new window at the pointer. A tab of a
   detached window released over the main window docks. This uses the pointer position winit reports
   outside a window during a drag on macOS. The target is the whole main window, not only its tab
   row, which is wider than was asked for.
6. **Restoring.** Windows are saved in the project's layout (`detachedWindows`) and come back with
   their tabs and bounds; a window whose files are gone stays closed.
7. **Opening from a window.** `editor.detachedWindow.openInMain` (Advanced Settings → Editor Tabs,
   on by default): a file a window does not hold opens in the main window, which comes to the front.
   Off, it opens in the window. It applies to every file opened from a detached window, not only
   declarations.

## Consequences

- Popups the main window draws (Search Everywhere, Find, dialogs) stay in the main window even when
  a shortcut is pressed in a detached one.
- Some behaviours of detached windows still wait for their checks.
