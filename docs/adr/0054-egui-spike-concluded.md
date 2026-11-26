# ADR-0054: The egui spike is concluded: the stack stays, accessibility is our work

- **Status:** Accepted
- **Date:** 2026-10-18

## Context

[ADR-0006](0006-desktop-first-ui-stack.md) chose egui on wgpu and left one question open: text
rendering, editor performance with large files and accessibility in egui "must be validated early",
in a Phase 2 spike. Since then the IDE window and the editor have been built on egui and released
in GoGarden, so most of the question was answered by practice, but nothing wrote the answer down.
The owner asked for the conclusion on 2026-10-18.

The spike's results are in [docs/behaviour/egui-spike.md](../behaviour/egui-spike.md):

- **Text:** at 200% GoGarden's editor text (the bundled editor font, 13) and UI text (Inter 13) have the
  UI specification's advances, x-height and rows to the pixel, and differ from its captures by a few
  per cent of coverage (slightly lighter stems; up to 2 px of drift over a long UI label). No 100%
  capture exists yet; at 100% egui hints and antialiases in grayscale and keeps every metric.
- **Performance:** `cargo xtask bench-editor` keeps typing at p95 11.3 ms and scrolling at 1.0 ms on
  a 100 000-line file ([docs/performance.md](../performance.md)).
- **Accessibility:** AccessKit is built in (eframe's default `accesskit` feature), and egui's own
  widgets, the native menu bar and the About panel are readable by screen readers. Our painted
  components and views (tabs, icon buttons, rows, the project tree, the editor) give no names or
  no nodes at all, so the window is mostly opaque to VoiceOver.

## Decision

1. **The UI stack stays egui on wgpu**, as ADR-0006 decided; the spike found nothing egui cannot do.
   ADR-0006's open question is answered by this record.
2. **Text rendering is checked against the UI specification's captures**, as in
   [docs/behaviour/egui-spike.md](../behaviour/egui-spike.md): the same strings, the same scale, ink
   extent, rows and coverage. The fonts, sizes, hinting and row heights are ours to set, so a
   difference is fixed in our code, not by changing toolkit.
3. **Screen reader support goes through AccessKit and stays on.** Every interactive element we
   draw ourselves gives `Response::widget_info` with its role and name, and the editor becomes an
   accessible multi-line text node (text runs, caret and selection). Until that work is done, the
   IDE is not accessible to screen reader users, and the documentation must not claim it is.

## Consequences

- No toolkit change and no migration; the layering of ADR-0004 keeps a replacement possible anyway.
- Accessibility becomes a work item of its own: naming the shared components first, then the
  painted views, then the editor, with a test that every input role has a name.
- Three checks remain open: a 100% capture for the per-pixel comparison at 1x, the editor's row
  height at 13 pt (22 pixels in the UI specification's capture, 24 in ours), and the stem weight.
- `deny.toml` keeps tolerating the advisories of AccessKit's Linux adapter until the egui upgrade
  that moves it to zbus 6, since turning the feature off is not an option.

## Alternatives considered

- **Turning AccessKit off** (`default-features = false` without `accesskit`) to drop its Linux
  dependencies and their advisories: rejected, it would close the only path to screen readers.
- **Another toolkit** (Slint, Iced, GPUI, native widgets) for better accessibility out of the box:
  rejected; their gains would not cover the editor, which is custom-drawn in any toolkit, and the
  rest is the same naming work on our side.
- **Editing ADR-0006** to record the result: not allowed for an accepted record; its status line
  links here instead.
