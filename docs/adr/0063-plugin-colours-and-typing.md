# ADR-0063: Plug-in ABI v1 additions: colour literals and what typing adds

- **Status:** Proposed
- **Date:** 2026-10-19
- **Extends:** [ADR-0056](0056-plugin-abi-v1.md) (ABI v1)

## Context

The web plug-ins of every IDE (ADR-0050) need two things for HTML and
style sheets: colour previews in the gutter for colour literals (`#ff8000`, `rgb()`, `hsl()`, named
colours), which a click turns into a colour picker; and closing tags inserted as you type (`>` of a
start tag adds its end tag, `</` gets the open element's name). Both depend on the language, so
they belong to plug-ins, and the plug-ins stay modules without imports. A new SCSS plug-in, from
the catalog and not bundled, reuses the CSS plug-in's code and needs the colours too.

## Decision

1. **Three optional exports in ABI v1**, not a new version. They are looked up by name, as the
   four of ADR-0056 are, and have their signature, `(ptr: i32, len: i32) -> i64`. A host that does
   not know them ignores them, so a module that has them still loads in an older IDE and the
   catalog's `abi` stays `1`; a module without them loses nothing.

   | Export | Input | Result |
   |---|---|---|
   | `zen_colors` | the UTF-8 text | records of 12 bytes, little-endian: `start: u32`, `end: u32` (byte offsets), `rgba: u32` (`0xRRGGBBAA`, not premultiplied) |
   | `zen_on_type_triggers` | nothing (a zero length) | UTF-8: the characters that make the host call `zen_on_type` |
   | `zen_on_type` | `caret: u32`, `typed: u32` (a Unicode scalar), then the text with the character typed; the caret is right after it | the JSON of `zen_expand`, `{"from", "to", "text"}`, with `$0` for the caret; a zero length for nothing |

2. **The host trusts nothing, as for ADR-0056.** Colour records out of order, overlapping, empty,
   out of bounds or not on character boundaries are dropped, at most 100 000 are taken, and a
   result that is not a list of 12-byte records switches the plug-in off. `zen_on_type_triggers` is
   called once when the module loads; at most 32 characters are kept, without control characters;
   if it traps the module is refused. `zen_on_type` is called only for those characters, only with
   a single caret and no selection; its answer must end at the caret, as an expansion must.
3. **The platform's side.** `zen_syntax::TextFeatures` gains `colors`, `reacts_to` and `on_type`
   with empty defaults, and `Feature::Colors` and `Feature::Typing`.
   - `editor.colors.list {path}` → `{version, colors: [{start, end, line, color: "#rrggbbaa", text}]}`
     (character offsets); for an open file the plug-in's answer is kept per document version, never
     asked for per frame; the editor reads it again when the text changed and typing has paused.
   - `editor.colors.set {path, offset, color}` replaces the literal at `offset` with `color`,
     written in the literal's own notation (hexadecimal in its case, `rgb()`/`rgba()`,
     `hsl()`/`hsla()`, commas or spaces as it had them; a named colour becomes hexadecimal), as one
     undo step.
   - `editor.edit.type` with one character asks the file's plug-in after typing it and applies the
     answer as its own undo step, so Undo takes back the closing tag first. The plug-in is asked
     only for its trigger characters, with the fuel of any other call; the typed character is in
     the text before the plug-in runs, so a plug-in that traps or runs out of fuel is switched off
     and the character stays.
4. **The editor.** The gutter shows one swatch per line, the first colour of the line, in the room
   for icons right of the line numbers (a run icon or the bulb wins it); a click opens egui's colour
   picker (`color_picker_hsva_2d`), Enter or a click elsewhere writes the colour, Escape keeps the
   old one. No inline swatches in the text yet.
5. **Shared code.** `plugins/css/src/color.rs` (the parser and scanner of colour literals) is
   self-contained and included by the HTML and SCSS plug-ins with `#[path]`; the SCSS plug-in
   includes every module of the CSS plug-in and picks the dialect with a `DIALECT` constant at its
   crate root (`token::Dialect::Scss`).

## Consequences

- HTML 0.2.0 and CSS 0.2.0 implement the new exports; SCSS 0.1.0 is a catalog plug-in in every IDE.
- egui's colour picker shows its own English hover texts (Hue, Saturation, Value, Alpha); the
  picker is not translated until we draw our own.
- Settings → Editor → Smart Keys: HTML/CSS → "Insert closing tag on tag completion" (session
  setting `editor.smartKeys.closingTags`, on by default) switches off everything a plug-in adds as
  you type, both the end tag after `>` and the name after `</`; a separate switch for `</`
  does not exist yet.
- Open: `lab()`, `lch()`, `oklab()`, `oklch()`, `hwb()` and `color()` are not previewed; Less is not
  supported.

## Alternatives considered

- **ABI v2:** a new version would refuse the bundled HTML and CSS modules in an IDE that knows only
  v1 when the catalog updates them; optional exports keep every IDE working.
- **Trigger characters in `plugin.json`:** they would need the catalog, the bundle and the
  installed manifest to carry them (ADR-0056 left that open); an export keeps them with the code.
- **Colours computed by the host:** the host would need to know every language's syntax (a `#fff`
  is an ID selector in CSS, a colour only in a value); the plug-in knows it.
