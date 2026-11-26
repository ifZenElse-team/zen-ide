# ADR-0039: The main menu is built from keymap actions; on macOS it is the native menu bar

- **Status:** Accepted
- **Date:** 2026-10-17

## Context

An IDE needs a main menu (File, Edit, View, Navigate, Code, Refactor, Build, Run, Tools, Git,
Window, Help); on macOS it is the system menu bar. Every item is an action with a name and a
shortcut. Zen IDE had no menu: eframe's default macOS menu holds only the application items.

## Decision

1. **Model.** `zen_ui::ide::main_menu::MENUS` lists the menus in their order,
   each item a keymap action (ADR-0038), optionally under a menu-specific name (`File…` in
   Navigate), with separators and submenus. An item whose action does not exist yet is left out, a
   menu left empty is left out, and separators never lead, trail or repeat. The item's name,
   shortcut and command are the keymap's; there is no second list of shortcuts.
2. **macOS: the native menu bar through `muda`** (re-exported by `tray-icon`, already a dependency
   for the Toolbox tray; pure Rust over `objc2` on macOS, no new crate). It replaces eframe's
   default menu and keeps the standard application menu (About, Settings…, Services, Hide, Hide
   Others, Show All, Quit) and the Window menu's Minimize and Zoom. The menu is rebuilt when the
   interface language or the keymap changes.
3. **Enabled by availability.** The focused project window enables each item by whether its
   command is available there (`view.*` actions always are). With no project window open, the
   items are disabled for now.
4. **An item runs as its shortcut.** macOS gives key equivalents to the menu before the window, so
   a shortcut shown in the menu always arrives as a menu event. The focused window turns the event
   into the item's keystroke in its own input, before it reads its shortcuts: a focused text field,
   the editor and global bindings then get it exactly as the keymap routes a key press (ADR-0038).
   An item without a shortcut runs its action directly. Clipboard items (Cut, Copy, Paste) are not
   in the menu until they are commands, so ⌘C, ⌘X and ⌘V keep reaching the window.
5. **Windows and Linux** draw the main menu inside the window; that comes with those platforms.

## Consequences

- A new menu item is one line in `MENUS`; its shortcut and name come from the keymap.
- Menus without actions yet (Refactor, Build, Run, Tools, Git) appear when their first action
  exists.
