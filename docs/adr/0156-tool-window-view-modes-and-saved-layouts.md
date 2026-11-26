# ADR-0156: Tool window view modes and layouts saved per IDE

- **Status:** Proposed
- **Date:** 2026-10-25
- **Extends:** [ADR-0033](0033-ai-access-to-every-api.md)

## Context

Every tool window was docked and pinned: shown at its side next to the editor until hidden. The
requirement: each tool window has a **View Mode** (⋮ in its header and Window → Active Tool Window →
View Mode, not its stripe button's context menu, which has only Hide, Move to and Show Tool Window
Names): **Dock Pinned**, **Dock Unpinned**, **Undock**, **Float** and **Window**. The current layout
is kept per project, and the layouts saved with Window → Layouts → Save Current Layout as New… per
IDE.

## Decision

1. **The mode is part of a tool window's layout.** `ToolWindowState` gains `mode` (`dockPinned`
   by default, `dockUnpinned`, `undock`, `float`, `window`) and `bounds` (`{ x, y, width, height }`
   in points, the outer position and the inner size of a floating or windowed one). Both are kept
   in the project's `workspace.json` with the rest of the layout; a file without them reads as
   docked and pinned. Bounds that are not finite are dropped, a size is at least 200 × 120.
2. **Behaviour.**
   - Dock Pinned and Dock Unpinned take their side's room next to the editor; one tool window per
     side, as before. An unpinned one hides when the pointer is pressed outside it (not on a
     stripe, not while a popup or dialog is open) and when another tool window is opened.
   - Undock draws the tool window over the editor at its side, as wide (or high) as its edge's
     size, in a layer above the editor; the editor keeps its room. It hides when another tool
     window is opened, not on a click in the editor.
   - Float and Window draw the tool window in an egui deferred viewport of its own, an OS window
     titled with the tool window's name. Float is a utility window above the IDE's windows while
     the IDE has the focus, with no Dock or taskbar entry and no minimise button; Window is an
     ordinary window. Each remembers where it is, on any screen, and opens there again; its close
     button hides the tool window. A backend with one window draws them inside the main window.
3. **Saved layouts per IDE.** `window-layouts.json` in the IDE's data folder holds
   `{ layouts: [{ name, layout }], current }`: every tool window's side, order, size, visibility,
   mode and bounds, and the layout applied or saved last (none for the default). Names are unique
   regardless of case and never `default`. Window → Layouts is: Default, then each
   saved layout as a submenu — Restore (⇧F12 on the current one),
   Save Changes, Rename, then Delete… (disabled for the current one) — then Save Current Layout as
   New…. The current layout has a check (the system's menu image on a submenu). There is no
   Restore Default Layout item and no Store Current Layout as Default; ⇧F12 restores the current
   layout. Applying a layout replaces the project's current layout, modes included; Default resets
   every mode to Dock Pinned. Save Changes writes the current tool windows into a saved layout;
   Rename and Delete… open small dialogs of our own.
4. **Commands** (ADR-0033): `view.toolWindow.viewMode { id?, mode }` and the keymap actions
   `view.toolWindow.viewMode/<mode>` (no shortcuts);
   `view.layout.saveAsNew { name? }` (without a name it opens its dialog, `saveLayout`),
   `view.layout.apply { name }`, `view.layout.restore { name? }` (⇧F12, the current layout by
   default), `view.layout.saveChanges { name? }`, `view.layout.rename { name?, newName? }` (without
   a new name it opens its dialog, `renameLayout`) and `view.layout.delete { name, ask? }` (never
   the current layout; `ask` opens its confirmation, `deleteLayout`). `view.layout.restoreDefault`
   stays for agents without a shortcut. `get_window_state` gives each tool window's `mode` and
   `bounds` and `toolWindows.layouts: { saved, current }`.

## Consequences

- Agents and people get the same five modes; a tool window can sit on another screen.
- Every place that asks which tool window shows at a side distinguishes docked (takes room) from
  shown at the side (docked or undocked); detached tool windows leave their side free.
- Not done yet: Remove from Sidebar in ⋮ and Show Tool Window Names in the stripe button's menu.

## Alternatives considered

- **Floating tool windows as egui windows inside the main window.** Simpler, but they could not
  leave the main window or move to another screen, which the requirement asks for.
- **Saved layouts per project.** Kept per IDE, one layout serves every
  project; per project would also duplicate them in every `workspace.json`.
