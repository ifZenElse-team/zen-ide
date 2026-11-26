# ADR-0038: One keymap drives every shortcut, and the user's changes live in keymap.json

- **Status:** Accepted
- **Date:** 2026-10-17

## Context

Shortcuts were written into the window and the editor view as code (`consume_shortcut` calls and a
`match` on keys), so they could not be listed, changed or shown in hints, and egui's logical
matching let ⌘F also catch ⇧⌘F. The IDE needs Settings → Keymap, where every action shows its
shortcuts and the user adds, removes and resets them.

## Decision

1. **The model** is `zen_core::keymap` (L4, no UI toolkit): a `Keystroke` (⌃ ⌥ ⇧ ⌘ and a key named
   as egui names it), a `Binding` (an action, the command it runs with fixed parameters, a context:
   global or editor, and its keystrokes) and a `Keymap` (the defaults with the user's changes).
   Actions are command ids, with a suffix when one command has several bindings
   (`editor.caret.move/wordLeft+extend`); `view.*` actions belong to the window (dialogs, tool
   windows). A test checks that every other action's command is registered.
2. **The default keymap** is our macOS keymap, written by us (ADR-0002), in code. Windows and Linux keymaps come later.
3. **Matching is exact.** A key press matches a keystroke only with the same modifiers. Global
   bindings are taken out of the input before anything else sees it; editor bindings are the
   editor's, and an editor binding wins over a global one with the same keystroke while the editor
   has the keyboard.
4. **The user's changes** are saved as `keymap.json` in the IDE's data folder: an object from action
   to its keystrokes written as on macOS (`{"editor.line.duplicate": ["⌥⇧D"]}`); an empty list
   removes an action's keystrokes, and an action missing from the file has its defaults. Only the
   changes are saved, so new defaults reach users. A keystroke that cannot be read is skipped.
5. **Settings → Keymap** lists the actions in groups with search by name or keystroke; Add Keyboard
   Shortcut (double click or the context menu) records a keystroke and offers to remove it from the
   actions that already have it; Remove and Reset Shortcuts. Changes apply on OK or Apply.
6. **AI agents** (ADR-0033) read the keymap with the MCP tool `get_keymap` and change it with
   `set_keymap`, which saves at once. Hints take their shortcuts from the keymap.

## Consequences

- A new shortcut is one line in `Keymap::default_macos` plus a name in the locales; nothing else.
- Find Action and the main menu (the next work) show shortcuts from the same keymap.
- Keystroke sequences (two keystrokes) and ⇧⇧ are not modelled yet.
