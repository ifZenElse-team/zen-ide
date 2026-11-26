# ADR-0096: Shortcuts of two keystrokes, and the numeric keypad's `*` read from the window events

- **Status:** Proposed
- **Date:** 2026-10-22
- **Amends:** [ADR-0038](0038-keymap.md) (consequence "Keystroke sequences are not modelled yet")
- **Relates to:** [ADR-0039](0039-main-menu.md), [ADR-0051](0051-jetbrains-keymaps-and-idea-project-settings.md)

## Context

Code → Folding has Expand to Level 1–5 and Expand All to Level 1–5, which are bound to two
keystrokes in a row: the numeric keypad's `*` with ⌘ (or ⌥⌘; Ctrl and Ctrl+Shift on Windows and
Linux), then the digit. Our keymap had one keystroke per shortcut, and the
JetBrains keymap importer reported every `second-keystroke` as not carried over.

egui has no key for the keypad's `*`: egui-winit turns neither its logical key (`*`) nor its
physical one (`NumpadMultiply`) into a key event, and with ⌘ held it sends no text either, so the
press never reaches the IDE's input. On macOS the native menu bar (ADR-0039) cannot help: a menu
key equivalent is a single keystroke.

## Decision

1. **A keystroke may have a second one.** `zen_core::keymap::Keystroke` gets `then`, the keystroke
   that must follow. It is written with a comma and a space between the two, `⌘NumPad*, 1`, in
   `keymap.json`, named keymaps, `get_keymap` and `set_keymap`, so the file format only gains
   values. Lookup stays exact (ADR-0038): a key press alone never matches a shortcut of two
   keystrokes; `Keymap::starts_sequence` tells whether a keystroke begins one.
2. **The editor waits for the second keystroke, briefly.** When a key press begins an editor
   shortcut of two keystrokes, the editor keeps it for two seconds and takes the next key press as
   the second one: a match runs the action and that key's text is not typed; anything else, or a
   key pressed later, is handled as if no first keystroke had come (a letter is typed, a shortcut
   runs), so a `*` pressed by mistake loses nothing. While the editor waits, the window's own
   shortcuts leave it the key that completes its shortcut. Only shortcuts of the editor's context
   start a wait; global shortcuts of two keystrokes are not used yet.
3. **The keypad's `*` is `NumPad*`** (key name `NumpadMultiply`). The IDE apps run their windows
   through `zen_ui::keypad::run_native`, which builds eframe's event loop as `eframe::run_native`
   does (`eframe::create_native`, `run_app_on_demand`) and sees every window event before eframe
   does: a press of `NumpadMultiply` is recorded with the modifiers held, on the UI thread, and the
   focused editor takes it as a keystroke in its next frame (presses not taken within a second are
   dropped). No `unsafe` and no new dependency: `eframe` and `winit` are already the apps'
   dependencies, and zen-ui (L6) may use them. `press_keys` (ADR-0033) presses `NumPad*` and both
   keystrokes of a shortcut the same way.

   **Why not something smaller.** eframe 0.37 offers no hook that sees the key: egui-winit maps
   neither the logical key `*` (`egui::Key::from_name` has no `*`) nor the physical
   `NumpadMultiply` to an `egui::Key`, and drops the text while ⌘ or Ctrl is held, so
   `App::raw_input_hook` and the input events never contain it; `NativeOptions::event_loop_builder`
   only configures the loop (on Windows a message hook could see the key, on macOS and Linux
   nothing could); an `NSEvent` monitor would need `unsafe` and new bindings for macOS only.

   **What stays as with `eframe::run_native`** (checked against eframe 0.37.0's
   `run_native` → `run_wgpu` with the default `run_and_return: true`): the event loop is built from
   the same `EventLoop::with_user_event()` builder with `NativeOptions::event_loop_builder` applied
   (so macOS's default menu and activation are winit's as before); `create_native` runs the same
   `init_native` (window title and app id from the app name when unset, the renderer) and wraps
   the same `WgpuWinitApp` in the same `WinitAppWrapper` with `run_and_return: true`, so the
   viewport (icon, size, restore), persistence, repaint scheduling, saving on exit and ⌘Q are the
   same code; the loop runs with `run_app_on_demand`, as `run_native` does. Every
   `ApplicationHandler` event is forwarded unchanged. The differences: the loop is not kept in
   eframe's thread-local for a second `run_native` (the apps run one); and eframe keeps the error
   that ended the loop to itself, so `run_native` here reports an error only when the app was
   never created (the creator's own error, or "the window could not start" when the graphics failed
   first), and returns `Ok` after an error later in the run, which eframe logs. An eframe upgrade
   has to keep `create_native` and `EframeWinitApplication`'s forwarding, and this list is checked
   again.
4. **Menus show no key equivalent for two keystrokes.** The native menu item of such an action has
   no accelerator (macOS menus cannot show two keystrokes); choosing it runs the action directly.
   Settings → Keymap and the website show both keystrokes.
5. **The JetBrains keymap importer carries `second-keystroke` over** as the second keystroke, and
   reads the keypad's `MULTIPLY`, `ADD` and `SUBTRACT`. A shortcut with a key we cannot name is
   reported as `unknownKey` with both keystrokes; `secondKeystroke` is no longer reported.

## Consequences

- Expand to Level and Expand All to Level have their two-keystroke shortcuts; any action can now get
  a shortcut of two keystrokes in a user's or an imported keymap.
- Settings → Keymap still records one keystroke at a time and cannot record the keypad's `*`.
- The windows no longer start through `eframe::run_native`; an eframe upgrade has to keep
  `create_native` and `EframeWinitApplication`'s event forwarding. A real keyboard and window are
  needed to check the start and the keypad's `*` by hand; tests cover what reads the key
  (`keypad::tapped`) and the error at start.
