# ADR-0137: Text with a preview; an SVG opens as text and as a picture

- **Status:** Proposed
- **Date:** 2026-10-24
- **Extends:** [ADR-0033](0033-ai-access-to-every-api.md),
  [ADR-0135](0135-file-types-by-content-and-xml-in-every-ide.md)

## Context

Every image, the SVG included, opened in the image viewer, so an SVG could not be edited. The
owner, 2026-10-24, asked for an `.svg` with two modes, text and picture, and a switch between them.
A text editor with a preview (Markdown, SVG) has a switch at the editor's top-right corner,
**Editor Only**, **Editor and Preview** and **Preview Only**; with both, the text is on the left and
the preview on the right with a divider between them; the preview follows the text as it is typed;
the SVG preview has the image viewer's toolbar (zoom, actual size, fit) over a chessboard.

## Decision

1. **Generic in the workbench** (`zen_workbench::preview`): a file has a preview kind by its name
   (`PreviewKind::of`; one kind today, `svg` for `*.svg`). Such a file opens as an ordinary text
   document (highlighting, problems, editing, saving as any text), and the window draws it as its
   kind's mode says. A kind is a renderer the platform has, so it is declared in the platform, not
   in `plugin.json`; a plug-in cannot draw a preview through the ABI yet.
2. **Three modes** (`PreviewMode`: `editorOnly`, `editorAndPreview`, `previewOnly`), kept in
   `previews.json` in the IDE's data folder. The layout is kept per file: the
   switch sets the mode of **its file**; the last 500 files switched are remembered. A file never
   switched shows its kind's mode: the one `editor.preview.setMode` with `scope: type` set (the
   Default layout setting), else the kind's default (Editor and Preview for SVG). The first `previews.json`, the modes of kinds alone, is read as the kinds' modes. The editor and preview share the width by a divider the user drags
   (15 %–85 %, kept for the window).
3. **Commands** (ADR-0033): `editor.preview.getMode { path?, waitMs? }` (kind, mode, and the
   preview's state: `ready`, `rendering`, `invalid` with `error`, `notOpen`, with the picture's
   size, and `scope`: `file`, `type` or `default`, where the mode comes from),
   `editor.preview.setMode { mode, path?, scope? }` (`file` by default, as the switch; `type` sets
   the kind's mode and drops that file's own), and the three modes as actions
   `editor.preview.editorOnly`, `editorAndPreview` and `previewOnly` (Find Action, the keymap's
   action ids; the file's mode). `get_window_state` has `preview: { kind, mode, scope }` for the active file;
   `editor.image.zoom` zooms the preview while it is shown.
4. **Drawing**: the preview is drawn from the document's current text, unsaved edits included, on a
   worker thread, one rendering at a time; when the text changed meanwhile, the newest text is
   rendered once the running one ends, and the last picture stays until then: typing never waits
   for a rendering, however large the SVG (the renderer caps a picture at 4 096 pixels a side, as the
   image viewer does; a text over 8 MiB shows a note). The SVG renderer is the image viewer's
   (`zen_image::decode_svg`, resvg, no fonts). A text that does not render shows why in the preview.
5. **Images**: only SVG changes (`zen_image::opens_as_picture`); PNG, JPEG, GIF, WebP, BMP and ICO
   still open in the image viewer.
6. **Our own**: the switch's three icons (lines; a split frame; a picture) are drawn by us; the
   preview's toolbar is the image viewer's, its zoom label after the buttons because the switch
   takes the corner.

## Consequences

- An SVG is edited as XML (the XML plug-in highlights and checks it, ADR-0135) with its picture
  beside it; MCP agents read and write it as text and ask the preview's state.
- Markdown or HTML previews can be added as more kinds of the same switch, with their renderers.
- A picture's text is rendered once per change, on a thread; a very large SVG is shown late, never
  slows typing.
