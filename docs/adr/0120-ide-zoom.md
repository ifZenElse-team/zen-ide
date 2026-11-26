# ADR-0120: Zoom IDE is a percentage on top of the UI font size

- **Status:** Proposed
- **Date:** 2026-10-22
- **Extends:** [ADR-0053](0053-fonts-ui-scale-and-code-style.md) (fonts and UI scale)

## Context

The IDE needs two separate ways to make its interface larger. Appearance → **Use custom font**
sets the size of the interface's text; View → Appearance → **Zoom IDE (Current: 100%)…** and
Appearance → Accessibility → **Zoom** scale the whole IDE, the editor included. The menu item has
no shortcut in the macOS keymap, and the zoom is kept per IDE.

ADR-0053 made the UI font size egui's zoom factor (`size / 13`) and divided the editor's font size and
line height by that factor, so that the editor keeps its size when the UI font changes. A second
factor that also scales the editor needs a place in that arithmetic.

## Decision

1. **`IdeSettings.ideZoom`** is the zoom in percent: 100 by default, any value from 50 to 300
   accepted. It is saved in the IDE's `settings.json`, so each IDE keeps its own.
2. **egui's zoom factor is `uiFontSize / 13 × ideZoom / 100`** (`IdeSettings::ui_zoom`). The editor's
   metrics divide by the font's part only (`IdeSettings::font_zoom`), so the UI font size still
   leaves the editor alone and the IDE zoom scales it with everything else. The IDE zoom in use is
   kept in the egui context next to the zoom factor it belongs to, so that the editor reads the pair
   egui is drawing with during the pass in which a new factor waits to take effect.
3. **The presets are 70, 80, 90, 100, 110, 125, 150, 175 and 200 %.** Zoom In and Zoom Out step to the
   next preset up or down (a custom value goes to the nearest preset in that direction) and stop at
   the ends; Reset goes to 100 %.
4. **The actions are window actions** (`view.ideZoom.open`, `view.ideZoom.in`, `view.ideZoom.out`,
   `view.ideZoom.reset`) in the keymap without default shortcuts, reachable by agents with
   `run_action`; any value is set with `set_settings { ideZoom }` (ADR-0033). The JetBrains keymap
   import maps `ZoomInIdeAction`, `ZoomOutIdeAction` and `ResetIdeScaleAction` to them; these
   ids are not yet confirmed by documentation or an imported keymap file [check].
5. **The dialog and the Settings row are our own design**: a modal
   with −, the percentage, +, Reset and Close, and a preset list with a separate Custom field and a
   Reset to default link shown only while the zoom is not 100 %.

## Consequences

- Settings files without `ideZoom` load at 100 %; nothing else changes for them.
- A headless IDE saves the zoom but draws nothing, so it has no effect there.
- The UI font size and the zoom multiply: the largest UI font at 300 % is a factor of about 7.4.

## Alternatives considered

- **The zoom as the UI font size:** one setting would hide the other, and the editor could not be
  scaled with the interface without changing its own font size.
- **The zoom as a factor (1.1) instead of a percentage:** the UI and the menu show
  percentages; an integer avoids rounding when stepping through the presets.
