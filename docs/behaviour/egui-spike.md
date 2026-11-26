# The egui spike: can egui reproduce our specified UI precisely?

> **Date:** 2026-10-18 · **Decision:** [ADR-0054](../adr/0054-egui-spike-concluded.md), concluding
> the spike [ADR-0006](../adr/0006-desktop-first-ui-stack.md) asked for · egui and eframe 0.37

ADR-0006 chose egui on wgpu and asked for an early spike on text rendering, editor performance with
large files and accessibility. This is its written result. Short answer: **yes for text, metrics and
performance; accessibility is possible with egui, and our views now give names (section 4)**.

## 1. Prototype at the specified metrics

Answered by practice: the IDE window and the editor shipped in GoGarden are built on egui at the
metrics recorded in [ide/main-window.md](ide/main-window.md) and [ide/welcome.md](ide/welcome.md).

## 2. Text rendering

### Method

- `cargo run -p zen-ui --example text_rendering -- <folder>` draws test strings with the editor's
  and the UI's fonts, sizes and row height through `zen_ui::offscreen` at 100% and 200%
  ([ide/text-rendering/](ide/text-rendering/)); a `ZEN_UI_SCREENSHOT` snapshot of GoGarden on a
  Retina Mac was checked against it.
- Measured per pixel row and column: the ink extent, the cap, x-height and descender rows, and the
  coverage (each crop's brightness normalised between its background and its brightest pixel, so
  that different text colours compare).

### At 200%

| Text | Width | Rows above the x-height / x-height |
|---|---|---|
| Editor: `Stack: Go 1.26, Fiber v3, GORM + pgxpool, Postgres, goose-миграции.` (the editor font, 13) | 1 037 px; advance 7.85 pt per character | 5 / 15 px |
| UI: the tab title `ai_scoring_instructions.md` (Inter 13) | 323 px | 5 / 15 px |

Both fonts (the editor font and Inter, bundled under the SIL OFL) are drawn at their nominal sizes
with grayscale antialiasing (macOS has had no subpixel antialiasing since 10.14)
([gogarden-editor@2x.png](ide/text-rendering/gogarden-editor@2x.png)).

**Findings:**

1. **Stem weight:** egui's text is light; if stems must be heavier, egui's `FontTweak` (hinting
   target) and the colour transfer function are the knobs.
2. **UI text drifts by up to 2 px over a long word** (0.6%): egui positions glyphs from the shaper's
   advances with sub-pixel binning. Labels keep their size.
3. **Ligatures:** egui's shaper (harfrust) applies the font's default features, so the editor font's
   ligatures (`<=` as `≤`, `::` tightened) appear in any text egui draws as is. The editor turns
   them off by default and on with Settings → Editor → Enable ligatures; other places that draw
   code in the monospace font (previews, popups) must do the same.
4. **The editor's line height:** our editor uses 24-pixel rows at 13 pt
   ([ide/main-window.md](ide/main-window.md)); 22-pixel rows at 13 pt are the alternative to
   verify with known font settings (Editor → Font: 13, line height 1.2). It is not a limit of egui:
   the row height is our constant (`LINE_HEIGHT` in `zen_ui::ide`).

### At 100%

- Our 100% rendering ([text@1x.png](ide/text-rendering/text@1x.png)) keeps every advance and row
  height of 200% (all metrics are in points), so the layout is the same at both scales.
- At 100% egui hints the outlines (skrifa's smooth hinting, vertical only, `TextOptions::font_hinting`
  on by default) and antialiases in grayscale; the result is crisp, with stems snapped to whole
  pixels.
- A per-pixel check at 100% needs a 1x display (an external monitor). Nothing in egui stands in the
  way: the fonts, sizes and hinting are all ours to set.

## 3. Typing latency and scrolling

Measured with `cargo xtask bench-editor` ([../performance.md](../performance.md), baseline
2026-10-17): at 100 000 lines typing takes 11.3 ms at p95 and scrolling 1.0 ms, within the
budget of 16 ms and 8 ms. Scrolling does not depend on the file's size; typing grows with it
because the whole text is highlighted again after each edit, which incremental highlighting
removes. egui's immediate mode is not the bottleneck.

## 4. Accessibility (screen readers)

Assessed from the code and the documentation; VoiceOver was not run.

**What is in place.** eframe's default features include `accesskit`, and every IDE and Zen Toolbox
builds with it (`cargo tree -i accesskit_macos` shows it under eframe). egui builds the
accessibility tree only when a screen reader asks for it, so it costs nothing otherwise. With
VoiceOver on, macOS would get:

- the native menu bar (`muda`, a real `NSMenu`) and the About panel: fully readable;
- egui's own widgets, with their names: `ui.label`, `ui.button`, check boxes, combo boxes and
  `TextEdit` (text fields, with their value and the caret);
- our toggle, which already names itself (`components::toggle`, role `Switch`).

**What is missing.** Most of the IDE draws with the painter and takes clicks with `ui.interact`,
without `Response::widget_info`. An AccessKit dump of the base components (`Context::enable_accesskit`,
one frame of the component gallery) shows:

| Component | Role in the tree | Name |
|---|---|---|
| `components::tabs`, `icon_button`, `list_row`, `primary_button` | `Unknown` | none |
| `components::text_field` | `TextInput` (value and caret) | none: the hint is not its label |
| `components::toggle` | `Switch` | its label |
| `ui.label`, `ui.button` | `Label`, `Button` | their text |
| text drawn with `painter.text` / `painter.galley` | absent | |

So VoiceOver can find the window and the menus, but in the window it meets unnamed elements: the
project tree's rows, editor tabs, tool window stripes, toolbar buttons and popups are drawn text
with no node or a nameless one. **The editor is invisible to it**: the text, the caret, the
selection and the problems are painted, and its keyboard input comes as raw events.

**What it would take** (no change of stack; AccessKit is the same library other Rust toolkits use):

1. Name every interactive element: `widget_info` with the right role (`Button`, `Tab`, `TreeItem`
   with level, expanded and selected state, `MenuItem`, `ListBoxOption`) in `components` and in the
   painted views, and the hint as a text field's label. Mostly mechanical: a few days for the
   components, then each view. A test can keep it so, as `egui::accessibility::is_input` marks the
   roles that must have a name.
2. The editor as a `MultilineTextInput` node with text runs (egui's `TextEdit` shows how, in
   `text_selection::accesskit_text`): the visible lines plus a window around the caret, the caret and
   selection as a `TextSelection`, line numbers and problems as descriptions. A week or two.
3. Focus order and announcements: keyboard focus that moves between tool windows, and live regions for notifications and the run console.
4. Checking with VoiceOver on macOS, Orca on Linux (AccessKit's Linux adapter, `accesskit_unix`, is
   in the build; see the advisories noted in `deny.toml`) and Narrator on Windows.

**Status, 2026-10-19**: steps 1 and 2 are done, step 3 in part, step 4 is open.

- `zen_ui::accessibility` names what we draw: `components` (buttons, icon buttons and list rows
  through `icon_button_named` and `list_row_named`, whose names are made only while a screen reader
  reads; `icon_button` and `list_row` stay as they were for the callers that name the response
  themselves; tabs with their `TabList`, text fields named by their hints, the toggle) and every painted view of the
  IDE and Zen Toolbox: toolbar, stripes (pressed or not), tabs, the Project, Structure, Find,
  Problems, Run, Debug and Git trees and lists, popups, the run widget, the status bar, dialogs and
  the Settings pages (labels name their fields through `labelled_by`). `hint` names every icon
  button that has a hint. Tool window titles are headings, notifications and banners `Status` live
  regions (an announcement is cut at 300 characters), the Run console a `Log` with its shown lines.
  The console's new output is announced by `accessibility::Announcer`, a polite `Status` beside the
  log: only lines added since the last announcement, the last three of them, at most once a second,
  and none of the lines written before the screen reader came; the log itself is not live, so
  scrolling it or a burst of output does not flood the screen reader.
- The editor (`ide/editor_access.rs`) is a `MultilineTextInput` named after the file, with one text
  run per line for the lines around the view and the caret (100 on each side), and the primary
  selection as its `TextSelection` (an end outside those lines is moved to the nearest line kept).
  Columns are the document's characters (Unicode scalar values), the same unit as AccessKit's
  character indices, and each character's length is its UTF-8 length; a run holds a line without
  its line break (LF, CRLF or CR) and then `\n`. Soft-wrapped lines are one run as tall as their rows;
  folded lines are kept without bounds. The runs are built again only when the document's version or
  those lines change; each frame only sets their bounds.
- eframe enables AccessKit when a screen reader asks for the tree (`InitialTreeRequested`) and
  disables it when it goes away. Until then `accesskit_node_builder` returns `None`, no name is
  copied, no painted text is read and the editor keeps no runs (tests check each). Frame time of
  the project window with a 10 000-line file, release build, offscreen: 0.33 ms with and without a
  screen reader (205 nodes, 122 text runs).
- `ide/app/accessibility_tests.rs` builds the IDE window offscreen with AccessKit on: every tool
  window, the project tree, popups, dialogs and every Settings page; it fails on any node with an
  input role or a click action and no name, and checks the editor's text, caret and selection.
  The Toolbox, the components and the Find, Structure, Run and Debug tool windows have their own.
- Open: VoiceOver has not been run (the check list is in the guide, "Accessibility"); the
  Git workspace tab and the Git tool window's branches and changes panels; focus announcements
  between tool windows; the terminal's screen; setting the caret or the selection from a screen
  reader (`SetTextSelection`, `ReplaceSelectedText`).

## 5. Conclusion

egui draws our text at our metrics, and its rendering pipeline gives us every knob (fonts, hinting,
transfer function, layout) to tune it. It is fast enough for 100 000-line files. Its accessibility
support is real (AccessKit, on in our build), but our custom-drawn views do not use it yet; that is
our work, not a limit of the stack. **The stack stays egui on wgpu** ([ADR-0054](../adr/0054-egui-spike-concluded.md)).

Follow-ups: check the rendering at 100% on a 1x display; verify the editor's row height at 13 pt
(finding 4 above); try the IDE with VoiceOver and finish the screen reader support (section 4).
