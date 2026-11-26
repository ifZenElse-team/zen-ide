# ADR-0261: Mermaid diagrams in the Markdown preview, laid out in pure Rust

- **Status:** Proposed
- **Date:** 2026-11-06
- **Extends:** [ADR-0138](0138-markdown-preview.md) (its point 10: Mermaid was shown as source),
  [ADR-0137](0137-text-with-preview.md)

## Context

ADR-0138 left Mermaid blocks as source because no complete pure-Rust renderer was known in October
2026. The IDE draws natively (no web view, no JavaScript engine), has no C or C++ in the default
build, works offline, and allows only the licences in `deny.toml`. On 2026-11-06 these crates on
crates.io were looked at, the first two by rendering the same four diagrams, the others from their
metadata and dependency lists:

| Crate | What it is | Verdict |
|---|---|---|
| `mermaid-svg` 0.7.0 (MIT) | Parser, layout and SVG for 23 diagram types; only dependency `thiserror` | **Chosen.** No file system, no system fonts, no network; every colour and the font come from a `Theme`; `<text>` elements, not `foreignObject`, so `resvg` can draw them |
| `mermaid-rs-renderer` 0.3.1 (MIT) | Parser, layout and SVG | Good pictures, but it loads the system's fonts and writes a font cache to `~/.cache/mmdr` whatever the caller does |
| `merman` 0.9.3 (MIT OR Apache-2.0) | Parity-focused port, many crates | Not tried: several crates and a newer minimum Rust, and `mermaid-svg` already draws what is asked for |
| `flowmaid` 0.35.0 | Mermaid-like engine, no dependencies | GPL-3.0-or-later, refused by `deny.toml` |
| `selkie-rs` 0.3.0, `mmdflux` 2.6.1, `ferrigura` 0.4.0, `mermaid-render` 0.10.0 (MIT) | Other renderers | Not tried: more dependencies, or little use so far |
| `mermaid-little` 11.14.0 (MIT) | Parity with upstream's output | Not tried: an optional feature pulls `rquickjs` (C) |

`zen-image` rendered SVG without text (`resvg` without its `text` feature, ADR-0137), and a diagram
is mostly labels.

## Decision

1. **Layout** lives behind one module, `zen_workbench::markdown::mermaid` (no UI): `svg` (source,
   palette, font family to SVG, or why not) and `draw` (to pixels at the screen's density). Replacing
   the crate touches this one module. A source over 256 KiB is refused unread, and so is one that nests
   more than 100 levels deep or indents a line by more than 400 characters, because a stack overflow
   ends the whole IDE and no catch sees it. The crate runs on a 64 MiB stack of its own; a panic in it is
   caught and shown as a diagram that cannot be read.
2. **Text**: `resvg`'s `text` feature is on, in pure Rust (`harfrust`, `fontdb`). `zen_image::SvgFonts`
   holds the faces a rendering may use and `render_svg_with_fonts` uses them; diagrams use the IDE's
   bundled Inter, regular and bold, never the system's. `decode_svg` and `render_svg` still have no
   fonts, so a user's SVG draws as it did (ADR-0137).
3. **Colours** come from the theme (text, secondary text, background, sticky background, accent,
   current line); the crate's dark or default theme, by the background, fills in the rest (pie slices,
   chart series). A diagram's own `%%{init}%%` may still change them, as it does elsewhere.
4. **Off the UI thread, cached:** as ADR-0225's formulas (`zen_ui::ide::markdown_mermaid`): one
   background thread per batch of new diagrams; the texture is kept per source, palette and density
   and dropped when unused for 600 frames. Until a diagram is ready its source is shown.
5. **Unsupported or broken** diagrams keep the source box, labelled Mermaid (source), with a note
   under it: "The diagram is not drawn: …". A diagram wider than the column shrinks to it.
6. **Selection**: as a display formula, a diagram takes part in the preview's selection as invisible
   text and copies as its fenced source. Accessibility names it with the role Image.
7. **Where it lives:** ADR-0050 puts Mermaid with the Markdown plug-in, but the plug-in ABI cannot
   draw a preview (ADR-0137, ADR-0138), so it stays in the platform's preview, with the rest of it.
8. **For agents**: `markdown.mermaid.list { path?, svg? }` gives each block's line, whether it is
   drawn, why not, and its SVG.
9. **Formatting** (the same request, a separate matter from drawing): the commands
   `markdown.format.bold`, `.italic`, `.strikethrough`, `.code`, `.link`, `.heading` and `.list`
   edit the text in `zen_workbench::markdown::format`. A new keymap context, `markdown`, holds their
   shortcuts: it is the context while a Markdown editor has the keyboard, and it comes before the
   global and editor bindings, so ⌘B formats there and still goes to the declaration everywhere else.
   Italic is written with `*`.

## Consequences

- A README's diagrams read as on the web, offline, drawn in the theme.
- `mermaid-svg` and `unicode-vo` join the lock file; `resvg`'s text uses `harfrust`, `fontdb` and
  `skrifa`, already there. The fonts are the UI's own, not a second copy.
- The layout is as complete as `mermaid-svg`; it is not pixel-equal to upstream's. Characters Inter
  lacks (CJK, for example) are not drawn in a diagram's labels, because diagrams use no system fonts.
- PlantUML stays out, as required: it has no pure-Rust renderer.
