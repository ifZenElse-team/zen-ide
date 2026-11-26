# ADR-0225: Math in the Markdown preview, typeset in pure Rust

- **Status:** Proposed
- **Date:** 2026-11-05
- **Extends:** [ADR-0138](0138-markdown-preview.md) (its point 10: math was shown as source)

## Context

ADR-0138 left TeX math (`$…$`, `$$…$$`) as source, because no pure-Rust typesetter that fits egui
was known in October 2026. The IDE draws natively (no web view, no JavaScript engine, no C or C++ in
the default build), works offline, and allows only the licences in `deny.toml` (fonts as unmodified
data under the SIL OFL with their notices). On 2026-11-05 these crates on crates.io were looked at:

| Crate | What it is | Verdict |
|---|---|---|
| `latex-rust` 3.0.1 (MIT OR Apache-2.0) | Parser, TeX layout from the OpenType MATH table of an embedded STIX Two Math (OFL), SVG/PNG output, egui shapes behind a feature | **Chosen.** Pure Rust, one dependency (`skrifa`), broad coverage (fractions, scripts, roots, limits, `\left…\right`, matrices, `aligned`, `cases`, `\text`, styled alphabets, colours), typed errors. Its `egui` feature pins egui 0.36.2, so it is not used: the box tree and its SVG are |
| `mathtex` 0.2.0 (MIT OR Apache-2.0) | Layout to a neutral IR and SVG | Young (0.2), smaller coverage, several crates |
| `katex-rs` 0.3.0 (MIT) | KaTeX's parser and HTML output in Rust | Produces HTML/CSS, needs a browser to lay it out |
| `katex-native` 0.1.1 (MIT AND OFL-1.1) | KaTeX's CSS applied natively | 0.1, depends on `katex-rs`; licence expression includes fonts in the crate itself |
| `leaf-math` 0.5.0 / RaTeX | TeX to an SVG picture through RaTeX | Thin wrapper; RaTeX crates at 0.1 |
| `formulary` 0.3.0, `math-core` 0.9.0, `tex2math` | MathML layout, or LaTeX to MathML | Need a MathML front end or a renderer we do not have |
| `fmd-math` 0.3.0 | TeX math layout | No licence declared |
| `katex` 0.4 | Bindings to KaTeX | Runs JavaScript (QuickJS, C) |

The fonts the IDE ships (Inter, the editor font, egui's fallbacks) have Greek and many operators,
but no OpenType MATH table: no stretchy delimiters, radicals or big operators. Writing our own
layout on them would give a weaker result than a maintained MATH-table typesetter.

## Decision

1. **Typesetting** lives behind a small interface in `zen_workbench::markdown::math` (no UI):
   `check`, `measure` (width, ascent, descent in points), `pieces` (each glyph, rule and line with
   its place, for tests) and `typeset` (metrics and a picture). It uses `latex-rust` for parsing and
   layout and renders its SVG with the IDE's own SVG renderer (`zen_image::render_svg`, resvg) at the
   screen's pixel density, in the theme's text colour. Replacing the crate touches this one module.
2. **Inline math** is set in the line at 1.15 times the text size, on the text's baseline; a tall
   formula makes its line taller. **Display math** (`$$…$$` and fenced blocks labelled `math`, which
   the reader now gives as math blocks) is centred at 1.3 times the body size, shrunk (down to 40%)
   when it is wider than the column.
3. **Off the UI thread, cached:** formulas are typeset on one background thread per batch of new
   formulas (inline on WebAssembly); the result is kept per formula, size, density and colour as a
   texture and dropped when unused for 600 frames. Until a formula is ready its source is shown.
4. **Errors** show the source: inline in the constant colour, underlined in the error colour, with
   the reason as a tooltip; a display formula in its source box labelled TeX, with a note under it
   ("The formula is not typeset: …").
5. **Selection** (ADR-0138's follow-up): each formula takes part in the preview's
   selection as invisible text spread over the picture, so dragging, double and triple clicks,
   ⌘A and ⌘C include it; it copies as `$…$` inline and `$$ … $$` for display math. Accessibility
   names a display formula with the role Math and its TeX.
6. **Licences:** STIX Two Math is compiled in through `latex-rust`, unmodified, under the SIL OFL
   1.1 (`assets/fonts/OFL-STIXTwoMath.txt`, NOTICE, ATTRIBUTION.md). The crate also carries the
   dvips colour names (a marked modified file, LaTeX Project Public License 1.3c) used by
   `\color{…}`; NOTICE says so.

## Consequences

- READMEs with formulas read as on GitHub, offline; agents keep reading the TeX through
  `markdown.preview.render`.
- The binary grows by the math face (about 0.8 MB) and the crate; `skrifa` 0.39 joins the lock file
  beside the 0.44 egui uses (a duplicate `cargo deny` warns about).
- The layout is as complete as `latex-rust`: an unknown command is an error shown as source, never
  a guess. Mermaid stays as source.
