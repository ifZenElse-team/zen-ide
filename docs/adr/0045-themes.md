# ADR-0045: Themes: model, file format, inheritance and importers

- **Status:** Proposed
- **Date:** 2026-10-18

## Context

The IDE's colours are constants in the UI crate: `IdePalette::DARK` and `IdePalette::syntax` in
`zen-ui`'s `ide/mod.rs`, about forty colour literals spread over the other `ide/*.rs` files, and
`TerminalPalette::DARK`. Nobody can change them, and italics of code are hard-coded. The owner
asked for a Zen theme by default, custom themes, and themes imported from JetBrains IDEs and
VS Code. That needs a model of a theme that does not depend on egui, a file format people can write
by hand, a way for one theme to change another, and a place for importers. ADR-0011 already gives
JetBrains formats to `zen-jb-import`.

## Decision

1. **A new crate, `zen-theme`** (`crates/platform/theme`), in layer **L1**. It depends only on
   serde, serde_json and schemars, uses no UI toolkit, compiles for WebAssembly and reads no files:
   callers pass text in. Every layer above may use it; the UI turns a resolved theme into its own
   colour type.
2. **Typed keys with stable ids.** A theme has four sections, each an enum with a stable string id
   and a one-line meaning per key: `UiKey` (`ui`, the window), `EditorKey` (`editor`), `SyntaxKey`
   (`syntax`: every `zen_syntax::HighlightKind`, the language server's `semantic.package`,
   `semantic.type`, `semantic.builtinType` and `semantic.call`, and `default`) and `TerminalKey` (`terminal`:
   background, foreground, cursor and the 16 ANSI colours). Colours are `Rgba`, written `#RGB`,
   `#RRGGBB` or `#RRGGBBAA`. A syntax key holds a `Style` (colour, optional background, bold,
   italic, an optional straight or wavy underline, strikethrough), so today's fixed italics become
   data. Project colours are the indexed keys `project.0` to `project.5`.
3. **Zen Dark is built in** (`zen-dark`) and is exactly the colours the IDE drew with before themes,
   as constant tables in the crate. A pin test (`crates/platform/theme/tests/zen_dark_pin.rs`)
   lists every key with its expected value and the place in `zen-ui` it came from; switching the UI
   to themes is checked against it. Where one constant of the old palette served two purposes (the
   island colour under the editor, the list selection in the editor, plain code), each purpose has
   its own key with the same value.
4. **Inheritance.** A theme is sparse: it sets only the keys it changes and names a `parent` by id.
   `Theme::resolve` follows the parents (another theme file, or a built-in theme), at most 8 of
   them, and puts Zen Dark below the last one, so the result, `ResolvedTheme`, has every key. Light
   themes fall back to Zen Dark too until a built-in light theme exists; this is documented, not
   hidden. A missing parent, a cycle, a chain that is too deep and keys this version does not know
   are warnings, never failures: a theme always loads.
5. **The file format** is JSON with `//` and `/* */` comments and trailing commas, removed by a
   small pre-pass (`zen_theme::jsonc`, public so that the VS Code importer reuses it) that keeps
   every byte in place, so errors carry the line and column of the original text. The shape is
   `{formatVersion: 1, id?, name, dark, parent?, ui, editor, syntax, terminal}`; `id` defaults to a
   form of the name. The JSON Schema is generated with schemars and committed as
   `docs/themes/theme.schema.json`, Zen Dark is committed as `docs/themes/zen-dark.json`, and tests
   fail when either is out of date. The format is described in [docs/themes.md](../themes.md).
6. **Importers** implement `zen_theme::ThemeImporter` and return an `ImportReport`: the theme, how
   many source entries were mapped, and the names of those that were not. The JetBrains importer
   (`.icls`, `.theme.json`) belongs in `zen-jb-import` (ADR-0011), the VS Code importer in
   `zen-theme`. Importers map observed meanings to our keys; no theme file of another product is
   copied into the repository (ADR-0002).

## Consequences

- The UI can switch to themes key by key, verified against the pin test, without changing a colour.
- Every new colour in the UI needs a key, a meaning in the enum, a value in Zen Dark and a row in
  docs/themes.md (generated from the enums and checked by a test).
- The format is versioned; a change that existing files cannot follow needs `formatVersion` 2.
- Until there is a built-in light theme, a light theme that leaves keys out shows dark colours there.

## Alternatives considered

- **Keys as free strings:** no compile-time check that the UI asks for a key that exists, and no
  single list to document and pin.
- **TOML or plain JSON:** TOML is not what editors' theme files use and does not map to VS Code
  themes; plain JSON without comments is unfriendly to edit by hand.
- **Themes in `zen-ui`:** the model would depend on egui and could not be used by headless tools,
  importers or the WebAssembly build.
- **A full theme per file, no inheritance:** every small change would copy every key, and the copies
  would fall behind when keys are added.
