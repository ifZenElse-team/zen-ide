# ADR-0112: Macros record dispatched editor commands and play them back as one undo step

- **Status:** Proposed
- **Date:** 2026-10-22
- **Relates to:** [ADR-0007](0007-everything-is-a-command.md), [ADR-0033](0033-ai-access-to-every-api.md),
  [ADR-0038](0038-keymap.md), [ADR-0039](0039-main-menu.md)

## Context

The IDE records editor actions into macros from Edit → Macros: Play Back
Last Macro, Start/Stop Macro Recording, Edit Macros and Play Saved Macros…, none with a default
shortcut. Saved macros are kept per IDE and appear in Settings → Keymap under Macros and in Find
Action, so they can be given a shortcut. Every action
of ours is a registered command (ADR-0007), so a recording is a log of dispatched commands with
their parameters.

## Decision

1. **Recording lives in the registry.** `zen_core::macros::Recorder` is a session state.
   `Registry::dispatch` adds a command that ran to the recording while it is on, when
   `zen_core::macros::is_recorded` names it: the editor commands on the active document
   (`editor.edit.*`, `editor.caret.*`, `editor.selection.*`, `editor.line.*`, `editor.comment.*`,
   `editor.code.reformat`, `editor.imports.optimize`, `editor.brackets.goToMatching`) and saving
   (`file.document.save`, `file.document.saveAll`). Queries, window actions (`view.*`), popups,
   tool windows and dialogs are not recorded. `Registry::dispatch_unrecorded` runs a command
   without recording it; the window uses it for mouse clicks in the editor and for tool windows
   that move the caret (Problems, Find in Files): a macro records no mouse clicks.
   Consecutive plain typing (letters, digits, spaces, `_`) is joined into one step; brackets and
   quotes stay steps of their own, since typing one character pairs them.
   The window's own follow-ups dispatch unrecorded too: polling a reformat the language server
   answers later, and Reformat Code from the Project tool window.
   Not recorded either, because they depend on what a popup or the language server offers at that
   moment: choosing a completion item, ⌥⏎ intentions and quick-fixes, the ⌘J template list, Find
   next and previous, folding. The caret moves and typing around them are recorded; Copy is not a
   command and Cut records only the deletion.
2. **Whose commands a recording keeps.** `zen_core::macros::Source` says who dispatches now: the
   window (keys, menus, Find Action, toolbars, and the MCP tools `run_action` and `press_keys`,
   which drive the window as a person does) or an agent (`dispatch_command` and a command's own MCP
   tool, run through `zen_core::macros::as_agent`). A recording keeps only what the source that
   started it dispatches: an agent editing files while a person records never lands in the
   person's macro, and a person typing while an agent records never lands in the agent's. A
   headless IDE has no window, so there every recording is an agent's.
3. **Playback is queued by a command.** A command cannot dispatch others, so `editor.macro.play`
   and `editor.macro.playLast` queue the steps on the recorder and the registry runs them right
   after the command returns, before `dispatch` answers. A step that fails stops the playback;
   the command then fails with the code `macroStepFailed` and the step's index and command. Macros
   cannot be played while one is recorded or played, so a macro bound to a key cannot record or
   play itself. A macro holding a step that `is_recorded` does not name (a `macros.json` edited by
   hand, a step playing a macro, a window action) is refused with `macroStepFailed` before any step
   runs. Positions are kept within the document the macro plays in; a `path` a step names (only
   Reformat Code and Optimize Imports take one, from an agent) is used as given.
4. **One undo step per playback.** The typing group of the active document is closed when the
   playback starts and when it ends, so that typing before or after it never joins it; the steps'
   undo steps are joined into one (`Document::merge_undo_steps`) when the playback ends, unless the
   macro itself undoes or redoes. A step on a read-only document changes nothing, as typing there
   does. The UI specification does not confirm this behaviour yet; this is the choice until it does.
5. **Saved per IDE.** `zen_workbench::macros::Macros` keeps the saved macros in `macros.json` of
   the IDE's data folder (`{ "formatVersion": 1, "macros": [{ "name", "steps": [{ "command",
   "params" }] }] }`); a damaged file is kept as `macros.json.corrupt`. Stopping without a name
   keeps the temporary last macro in memory only; names of saved macros are unique.
6. **Keymap and Find Action.** Every saved macro is the keymap action `editor.macro.play/<name>`
   (group Macros, no default shortcut) running `editor.macro.play` with its name, so Settings →
   Keymap, Find Action, `get_keymap`/`set_keymap` and the window's shortcuts know it. Renaming a
   macro moves its shortcuts in every keymap; removing it drops them A name holds any characters (slashes,
   quotes, `+extend`); the action is the prefix followed by the trimmed name as it is.
7. **Commands for agents (ADR-0033).** `editor.macro.startRecording`, `stopRecording {name?}`,
   `playLast`, `play {name}`, `list`, `rename {name, newName}`, `remove {name}`,
   `removeStep {name, index}`; the window actions `view.macro.toggleRecording` (Stop opens Enter
   Macro Name, or stops at once with `{ name }`), `view.macro.edit` and `view.macro.playSaved`.

## Consequences

- Every command stays recordable without changes of its own; a new editor command under the
  prefixes above is recorded at once, and a new window-internal dispatch of one of them must use
  `dispatch_unrecorded`.
- A recorded caret move keeps the parameters it ran with, such as `visual` in a soft-wrapped file.
- Formatting answered later by a language server lands after the playback and stays an undo step
  of its own.
- Autosave (ADR-0110) saves through `file.document.autosave`, which `is_recorded` does not
  name, so an idle or frame-deactivation save never lands in a macro; merging it needs no change
  here. Its `save_projects` (exit, closing a project) dispatches `file.document.saveAll` as the
  window did before; that step only ever ends a session, and a recording ends with it.
  Merged with this change, `crates/platform/ui/src/ide/app.rs` conflicts only in the list of test
  modules: keep both `mod macro_tests;` and `mod autosave_tests;`, each under its own
  `#[cfg(all(test, not(target_arch = "wasm32")))]`.
- The window keeps one keymap store per process (`zen_ui::keymap::load`), the one `Macros` updates;
  tests that start several IDEs in one process load their own store before reading it.
