# ADR-0037: Images open in a built-in image viewer

- **Status:** Accepted
- **Date:** 2026-10-16

## Context

The owner (2026-10-16) asked for a module that opens pictures, together with pasting screenshots
into the project. Image files could not be opened at all: the editor reads files as UTF-8 text.
Images need a viewer with zoom, a chessboard behind transparent pixels and the image's size, format
and colour depth.

## Decision

1. **Decoding** is a new platform crate, `zen-image` (L0): PNG, JPEG, GIF (the first frame), WebP,
   BMP and ICO through the `image` crate with pure-Rust decoders only (`default-features = false`,
   so no threads and no C codecs), and SVG through `resvg`, which the platform already uses for
   icons. SVG is drawn without fonts, so text in an SVG is not shown. Files are recognised by their
   extension (`zen_image::EXTENSIONS`).
2. **Documents.** `file.document.open` opens an image file as an image document
   (`zen_workbench::ImageView`): the decoded picture and its zoom, with no text. Typing does nothing
   and saving leaves the file alone. Images open at their own size, or smaller to fit the window.
3. **Commands** (ADR-0007, ADR-0033): `editor.image.zoom` (`in`, `out`, `actual`, `fit`, or a
   `scale` from 0.02 to 32) and `editor.image.info` (size, format, bits per pixel, file size, zoom).
   The MCP tool `get_document_text` refuses images and points to `editor.image.info`.
4. **The view** (`zen-ui`, `ide/image_view.rs`): a toolbar with Zoom In (⌘=), Zoom Out (⌘−),
   Actual Size (⌘/) and Fit Zoom to Window, the zoom and the image's details; the picture centred
   over a chessboard, scrolled and dragged when larger than the window; ⌘ with the scroll wheel or
   a pinch zooms. Raster pixels stay sharp when zoomed in. Pictures larger than the graphics card's
   texture limit are shown shrunk.

## Consequences

- The `image` crate and its decoders (zune-jpeg, gif, image-webp) join the dependencies; all are
  pure Rust under licences the policy allows.
- Animated GIFs show their first frame; editing images is out of scope.
