# ADR-0154: Quick Switch Scheme switches through the commands and settings of Settings

- **Status:** Proposed
- **Date:** 2026-10-22
- **Relates to:** [ADR-0007](0007-everything-is-a-command.md),
  [ADR-0033](0033-ai-access-to-every-api.md), [ADR-0038](0038-keymap.md), [ADR-0045](0045-themes.md),
  [ADR-0051](0051-jetbrains-keymaps-and-idea-project-settings.md), [ADR-0053](0053-fonts-ui-scale-and-code-style.md)

## Context

Quick Switch Scheme is a popup titled "Switch", opened with ⌃` (Ctrl+` on Windows and Linux), from
View → Quick Switch Scheme… and from Find Action. The UI specification gives it seven numbered rows: Theme, Editor Color Scheme, Code Style
Scheme, Keymap, View Mode, Layout and Zoom. Choosing a row replaces the popup by a second popup
titled after the row, listing its choices; an arrow marks the choice in use in most lists, typing
filters the rows, and a second ⌃` does nothing. Some rows do not fit Zen IDE as it is:

- a Zen IDE theme sets the colours of the whole IDE, the editor's included (ADR-0045): there is no
  editor colour scheme apart from the theme;
- the code style is the user's (Editor → Code Style) with the project's own `.idea` code style on
  top when the project keeps one (ADR-0051, ADR-0053); there was no way to choose between them,
  and Zen IDE never writes `.idea`, so the choice cannot be kept there;
- Zen IDE has no saved window layouts (only Restore Default Layout), no Compact Mode and no
  Presentation Assistant, and no plug-in marketplace entries for themes or keymaps.

## Decision

1. **One popup, the same commands.** `view.quickSwitchScheme.open` (⌃`, group Windows, View →
   Quick Switch Scheme…, Find Action; `QuickChangeScheme` in an imported JetBrains keymap) opens the
   popup with six rows, numbered 1 to 6: Theme, Editor Color Scheme, Code
   Style Scheme, Keymap, View Mode and Zoom. Layout is left out until there are saved layouts. A
   choice runs exactly what Settings or the View menu runs: `ui.theme.select`, `ui.keymap.select`,
   `view.settings.open` on the Keymap page (Configure Keymap…), the View Mode actions
   (`view.presentationMode.toggle`, `view.distractionFreeMode.toggle`, `view.fullScreen.toggle`,
   `view.zenMode.toggle`, read Enter or Exit by the state as in the menu) and, for the code style
   scheme and the zoom, the settings Settings saves (`codeStyleScheme`, `ideZoom`). So
   `settings.json` ends up the same whichever way the choice is made. `run_action` with the action
   answers every row with its choices, which one is in use, and the command, action or setting
   each runs.
2. **Editor Color Scheme lists the themes.** Since the theme is the colour scheme, Editor Color
   Scheme and Theme list the same themes (built-in first, those that cannot be used left out) and
   both choose with `ui.theme.select`; the theme in use reads *Theme default* in Editor Color
   Scheme, which marks the scheme a theme brings. A separate editor colour scheme would
   be a change to ADR-0045 of its own.
3. **Code Style Scheme is a setting.** `codeStyleScheme` in `settings.json` is `project` (the
   default, the behaviour so far: a project's `.idea` code style wins where it sets one) or
   `default` (the user's own code style only). It is IDE-wide, shown as Editor → Code Style →
   Scheme in Settings, set by `set_settings`, and reaches the sessions as
   `editor.projectCodeStyle`, which `zen_workbench::project_settings::code_style_override` reads.
   The popup lists `<Project>`, then Default. Keeping the choice per project would need a place of
   our own outside `.idea`, which this decision leaves for later.
4. **Keys.** Enter, a click or a row's digit (0 the tenth; Zoom has no digits) shows
   that row's list in the popup's place; in a list the same keys choose at once and close the
   popup. → and ← do nothing. An arrow marks the choice in use in Editor Color Scheme, Code Style
   Scheme and Keymap, with the row after it selected; Theme and Zoom select the choice in use.
   Typing filters the rows, also in the other keyboard layout (`zen_index::layout`), with a search
   field at the top; Backspace edits the filter, Escape clears it and then closes the popup. ⌃`
   while the popup is open does nothing.

## Consequences

- One more popup in `get_window_state` (`quickSwitchScheme`) and one more setting for agents.
- ⌃` is free in the macOS keymap and in the terminal's keys only goes to the terminal, as other
  shortcuts without ⌘ do.
- Editor Color Scheme repeats Theme until there is a colour scheme of its own; the owner decides
  whether that is wanted.
- Layout, Get More Themes…, Install Keymap…, Compact Mode and Presentation Assistant wait for the
  features they would switch.
