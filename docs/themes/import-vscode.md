# Importing VS Code colour themes

`zen_theme::VsCodeImporter` turns a VS Code colour theme, the JSON file a VS Code extension lists
under `contributes.themes`, into a Zen [theme](../themes.md) whose parent is Zen Dark, or Zen Light for a light theme
([ADR-0045](../adr/0045-themes.md)). The importer is pure: it reads no files itself. The caller passes
the file's text, its path, and a function that returns the text of an included file.

```rust
let importer = VsCodeImporter::with_includes("themes/night-color-theme.json", |path| read(path));
let report = importer.import(&text)?;
// report.theme: the keys the file set; report.mapped and report.unmapped: what was carried over.
```

`VsCodeImporter::new()` imports a file without includes: an `include` is then reported as not found.

## What is read

| Field | Becomes |
|---|---|
| `name` | The theme's name; its id is made from the name. A file without a name is called `VS Code theme`. |
| `type` | `dark`: `dark`, `hc`, `hcDark`, `hc-black`, `vs-dark` are dark; `light`, `hcLight`, `hc-light`, `vs` are light. Without `type`, a light `editor.background` makes a light theme. |
| `include` | A parent theme file, a path relative to the including file. |
| `colors` | Window, editor and terminal colours, by the [table below](#colours). |
| `tokenColors` | Code styles, from TextMate scope rules ([below](#token-colours)). |
| `semanticTokenColors` | Code styles from semantic tokens, which override `tokenColors` ([below](#semantic-token-colours)). |
| `semanticHighlighting` | When `false`, `semanticTokenColors` are not used. |
| `$schema` | Ignored. |

The file is JSON with comments and trailing commas, read with `zen_theme::jsonc::strip`, so a
malformed file is an error with the line and column of the original text. `tokenColors` given as a
path to a `.tmTheme` file is not read and is reported.

## Includes

An `include` is joined to the folder of the file that names it (`./` and `../` are resolved, `/`
separates folders) and loaded through the caller's function. The included theme is read first and
the including file on top of it: its `name`, `type`, colours and semantic token colours replace the
parent's, and its `tokenColors` rules come after the parent's, so they win ties. Includes may
include further files. A file that is already in the chain (a cycle), a file the function does not
return, and a chain of more than 8 includes end the chain there and are reported as unmapped
entries such as `include ./base.json (not found)`; an included file that is not valid JSON is an
error that names its path.

## Alpha

VS Code colours may be `#RGB`, `#RGBA`, `#RRGGBB` or `#RRGGBBAA`. A key that is drawn over other
content keeps the alpha: the selection, the current line, search matches, other uses of the symbol,
folded regions, hovered and selected rows, the popup hover, the popup shadow and the background of
code in popups (the `Alpha` column says `kept`). Every other key is opaque, and its colour is
composited over a background:

- `editor.background` over Zen Dark's editor background in a dark theme, over white in a light one;
- terminal colours over the terminal background, which is itself composited over the editor
  background (the editor background when the theme has no terminal background);
- every other colour, and the text colours of code styles, over the editor background.

The background of a code style (`settings.background`) keeps its alpha.

## Colours

When several VS Code colours map to one key, the first one in the table that the file sets wins
(for example `sideBar.background` over `titleBar.activeBackground` for `ui.window.background`).
A colour that is not in the table, or that cannot be read, is reported by its name; `null` is
skipped.

VS Code has no colour for a modified line of a diff, and many themes set only the gutter's change
markers. The diff colours a theme leaves out are then tints of those markers, painted over the
editor's background ([Derived diff colours](#derived-diff-colours)): the modified marker gives
`editor.diff.modified` and `editor.diff.modifiedWord`, the added and deleted markers the inserted
and deleted line colours the theme does not set. `diffEditor.diagonalFill` becomes the filler rows.

## Light themes

A light theme (`type` `light`, `hcLight`, `hc-light` or `vs`, or a light `editor.background`) builds
on Zen Light, a dark one on Zen Dark: every key the file does not set comes from the built-in theme
of the same appearance, so a light theme never shows a dark tree, a dark current line or dark popups.
Code styles without a colour fall back to the editor foreground, or to that base theme's colour.
When the theme is imported into the IDE's themes folder, text that would still be too faint on its
background is made readable ([themes.md](../themes.md#readability)).

## Token colours

A `tokenColors` rule has a `scope` (one selector, a comma-separated list, or an array of either)
and `settings` with `foreground`, `background` and `fontStyle`. Each Zen code style probes a short
list of representative TextMate scopes, in order; the first scope that any rule matches gives the
style.

- **Matching.** A selector matches a scope when it is the scope or a dotted prefix of it: `keyword`
  matches `keyword.control.go`, while `keyword.control` does not match `keyword`, and `key` does not
  match `keyword`. A selector of several words (`meta.function-call entity.name.function`) needs
  each earlier word to match an enclosing scope, in order; `a - b` matches what `a` matches unless
  `b` matches too. Selectors with `(`, `)`, `|`, `&`, `>` or `*` are not read and are reported.
- **Specificity.** Of the rules that match, the one matching the innermost scope wins, then the
  one with the most dotted segments in its last word, then the one with the most words. A tie goes
  to the later rule, so a file's rules beat those of the theme it includes.
- **Each setting separately.** The foreground, the background and the font style each come from
  the best matching rule that sets them, so a rule with only `fontStyle` keeps the colour of a less
  specific rule. A style without any foreground takes the editor foreground, or the base theme's colour
  for that key when the theme has none.
- **Font style.** `fontStyle` is a list of `bold`, `italic`, `underline` (a straight underline) and
  `strikethrough`; an empty string means none of them.
- **The global rule.** A rule without a `scope` sets editor colours that `colors` leaves out, as
  in the table of global settings; its `foreground` also becomes `syntax.default`.
- **Calls.** `syntax.semantic.call` probes `entity.name.function` inside `meta.function-call`: a theme
  with a rule for calls gives calls their own style, any other theme gives them the function style.

A selector that matches no probed scope (such as `variable.parameter` or `source.go`) is reported as
`tokenColors: <selector>`, and a rule without `settings` as `tokenColors: <its name>`.

## Semantic token colours

`semanticTokenColors` maps a selector (`type`, `type.modifier`, optionally `:language`) to a colour
or to an object with `foreground`, `fontStyle`, `bold`, `italic`, `underline` and `strikethrough`.
A selector in the table replaces the whole style that `tokenColors` gave the key; without a
`foreground` it keeps that style's colour. For each key the first selector of its list that the file
sets wins: `function.declaration` is preferred for `syntax.function`, while plain `function` also
colours calls (`syntax.semantic.call`). Selectors that are not in the table, such as `*.deprecated`
or `variable.readonly:go`, and values that cannot be read are reported.

## The report

`ImportReport::mapped` counts the source entries that became keys: colours of the table, settings
of the global rule, `tokenColors` selectors that match a probed scope, and semantic selectors of the
table. An entry counts even when another one wins its key. `ImportReport::unmapped` lists the rest
by their names in the file, each once: unknown top-level fields, unknown or unreadable colours,
unread global settings, `tokenColors: <selector>`, `semanticTokenColors: <selector>` and the
`include` problems above. The theme sets only the keys the file gave it (and the derived diff colours), so it resolves against its base
theme without warnings.

## Mapping tables

The tables are generated from `zen_theme::import::vscode` (`COLORS`, `GLOBAL_SETTINGS`, `SCOPES`,
`SEMANTIC`, `DERIVED`); `ZEN_THEME_BLESS=1 cargo test -p zen-theme --test vscode_import` rewrites them, and the
test fails when they are out of date. Keys are written with their section, as `ui.accent`.

<!-- vscode:begin -->

### Colours

| VS Code colour | Zen key | Alpha |
|---|---|---|
| `editor.background` | `editor.background` | composited |
| `editor.foreground` | `editor.foreground` | composited |
| `editorCursor.foreground` | `editor.caret` | composited |
| `editor.selectionBackground` | `editor.selection` | kept |
| `editor.lineHighlightBackground` | `editor.currentLine` | kept |
| `editorLineNumber.foreground` | `editor.lineNumber` | composited |
| `editor.findMatchBackground` | `editor.search.match` | kept |
| `editorOverviewRuler.findMatchForeground` | `editor.search.mark` | composited |
| `editor.findMatchHighlightBackground` | `editor.search.mark` | composited |
| `editor.foldBackground` | `editor.fold.marker` | kept |
| `editor.wordHighlightBackground` | `editor.symbolHighlight` | kept |
| `editor.wordHighlightStrongBackground` | `editor.symbolHighlight.write` | kept |
| `editorBracketMatch.background` | `editor.bracket.matched.background` | kept |
| `editorStickyScroll.background` | `editor.stickyLines.background` | composited |
| `editorStickyScroll.border` | `editor.stickyLines.border` | composited |
| `editorInlayHint.background` | `editor.inlayHint.background` | composited |
| `editorInlayHint.foreground` | `editor.inlayHint.foreground` | composited |
| `editorGutter.addedBackground` | `editor.gutter.added` | composited |
| `editorGutter.modifiedBackground` | `editor.gutter.modified` | composited |
| `editorGutter.deletedBackground` | `editor.gutter.deleted` | composited |
| `diffEditor.insertedLineBackground` | `editor.diff.inserted` | composited |
| `diffEditor.removedLineBackground` | `editor.diff.deleted` | composited |
| `diffEditor.insertedTextBackground` | `editor.diff.insertedWord` | composited |
| `diffEditor.removedTextBackground` | `editor.diff.deletedWord` | composited |
| `diffEditor.diagonalFill` | `editor.diff.filler` | composited |
| `editorWhitespace.foreground` | `editor.whitespace` | composited |
| `editorIndentGuide.background1` | `editor.indentGuide` | composited |
| `editorIndentGuide.background` | `editor.indentGuide` | composited |
| `editorIndentGuide.activeBackground1` | `editor.indentGuide.selected` | composited |
| `editorIndentGuide.activeBackground` | `editor.indentGuide.selected` | composited |
| `editorRuler.foreground` | `editor.rightMargin` | composited |
| `editorError.foreground` | `editor.diagnostic.error` | composited |
| `editorWarning.foreground` | `editor.diagnostic.warning` | composited |
| `editorInfo.foreground` | `editor.diagnostic.info` | composited |
| `editorHint.foreground` | `editor.diagnostic.hint` | composited |
| `sideBar.background` | `ui.window.background` | composited |
| `titleBar.activeBackground` | `ui.window.background` | composited |
| `statusBar.background` | `ui.window.background` | composited |
| `panel.background` | `ui.island.background` | composited |
| `editorGroupHeader.tabsBackground` | `ui.island.background` | composited |
| `foreground` | `ui.text.primary` | composited |
| `descriptionForeground` | `ui.text.secondary` | composited |
| `errorForeground` | `ui.text.error` | composited |
| `editorWarning.foreground` | `ui.text.warning` | composited |
| `textLink.foreground` | `ui.link` | composited |
| `button.background` | `ui.accent` | composited |
| `button.foreground` | `ui.accent.foreground` | composited |
| `list.hoverBackground` | `ui.list.hover` | kept |
| `list.activeSelectionBackground` | `ui.list.selection` | kept |
| `list.inactiveSelectionBackground` | `ui.list.selection` | kept |
| `editorWidget.background` | `ui.popup.background` | composited |
| `menu.background` | `ui.popup.background` | composited |
| `editorWidget.border` | `ui.popup.border` | composited |
| `menu.border` | `ui.popup.border` | composited |
| `menu.selectionBackground` | `ui.popup.hover` | kept |
| `quickInputList.focusBackground` | `ui.popup.hover` | kept |
| `widget.shadow` | `ui.popup.shadow` | kept |
| `textCodeBlock.background` | `ui.popup.code.background` | kept |
| `input.background` | `ui.textField.background` | composited |
| `input.border` | `ui.textField.border` | composited |
| `button.border` | `ui.button.secondary.border` | composited |
| `tab.border` | `ui.tab.border` | composited |
| `tab.activeBackground` | `ui.tab.active.background` | composited |
| `tab.activeBorderTop` | `ui.tab.active.border` | composited |
| `tab.activeBorder` | `ui.tab.active.border` | composited |
| `gitDecoration.modifiedResourceForeground` | `ui.tab.modified` | composited |
| `gitDecoration.ignoredResourceForeground` | `ui.tree.excluded.text` | composited |
| `gitDecoration.modifiedResourceForeground` | `ui.vcs.modified` | composited |
| `gitDecoration.addedResourceForeground` | `ui.vcs.added` | composited |
| `gitDecoration.deletedResourceForeground` | `ui.vcs.deleted` | composited |
| `gitDecoration.untrackedResourceForeground` | `ui.vcs.unversioned` | composited |
| `gitDecoration.ignoredResourceForeground` | `ui.vcs.ignored` | composited |
| `gitDecoration.conflictingResourceForeground` | `ui.vcs.conflict` | composited |
| `scmGraph.foreground1` | `ui.vcs.graph.0` | composited |
| `scmGraph.foreground2` | `ui.vcs.graph.1` | composited |
| `scmGraph.foreground3` | `ui.vcs.graph.2` | composited |
| `scmGraph.foreground4` | `ui.vcs.graph.3` | composited |
| `scmGraph.foreground5` | `ui.vcs.graph.4` | composited |
| `banner.background` | `ui.banner.background` | composited |
| `terminal.background` | `terminal.background` | composited |
| `terminal.foreground` | `terminal.foreground` | composited |
| `terminal.selectionBackground` | `terminal.selection` | composited |
| `terminalCursor.foreground` | `terminal.cursor` | composited |
| `terminal.ansiBlack` | `terminal.ansi.0` | composited |
| `terminal.ansiRed` | `terminal.ansi.1` | composited |
| `terminal.ansiGreen` | `terminal.ansi.2` | composited |
| `terminal.ansiYellow` | `terminal.ansi.3` | composited |
| `terminal.ansiBlue` | `terminal.ansi.4` | composited |
| `terminal.ansiMagenta` | `terminal.ansi.5` | composited |
| `terminal.ansiCyan` | `terminal.ansi.6` | composited |
| `terminal.ansiWhite` | `terminal.ansi.7` | composited |
| `terminal.ansiBrightBlack` | `terminal.ansi.8` | composited |
| `terminal.ansiBrightRed` | `terminal.ansi.9` | composited |
| `terminal.ansiBrightGreen` | `terminal.ansi.10` | composited |
| `terminal.ansiBrightYellow` | `terminal.ansi.11` | composited |
| `terminal.ansiBrightBlue` | `terminal.ansi.12` | composited |
| `terminal.ansiBrightMagenta` | `terminal.ansi.13` | composited |
| `terminal.ansiBrightCyan` | `terminal.ansi.14` | composited |
| `terminal.ansiBrightWhite` | `terminal.ansi.15` | composited |

### Derived diff colours

| VS Code colour | Zen key, when the theme does not set it | Alpha |
|---|---|---|
| `editorGutter.modifiedBackground` | `editor.diff.modified` | `40` of `FF`, over the editor |
| `editorGutter.modifiedBackground` | `editor.diff.modifiedWord` | `80` of `FF`, over the editor |
| `editorGutter.addedBackground` | `editor.diff.inserted` | `40` of `FF`, over the editor |
| `editorGutter.addedBackground` | `editor.diff.insertedWord` | `80` of `FF`, over the editor |
| `editorGutter.deletedBackground` | `editor.diff.deleted` | `40` of `FF`, over the editor |
| `editorGutter.deletedBackground` | `editor.diff.deletedWord` | `80` of `FF`, over the editor |

### The global rule

| Setting | Stands in for |
|---|---|
| `background` | `editor.background` |
| `foreground` | `editor.foreground` |
| `caret` | `editorCursor.foreground` |
| `selection` | `editor.selectionBackground` |
| `lineHighlight` | `editor.lineHighlightBackground` |

### Token colours

| Zen key | Scopes probed, in order |
|---|---|
| `syntax.keyword` | `keyword`, `keyword.control`, `storage.type`, `storage.modifier` |
| `syntax.string` | `string` |
| `syntax.number` | `constant.numeric` |
| `syntax.comment` | `comment` |
| `syntax.docComment` | `comment.block.documentation` |
| `syntax.macro` | `entity.name.function.macro` |
| `syntax.type` | `entity.name.type`, `entity.name.class`, `support.type` |
| `syntax.function` | `entity.name.function`, `support.function` |
| `syntax.lifetime` | `storage.modifier.lifetime` |
| `syntax.constant` | `variable.other.constant`, `constant.language` |
| `syntax.attribute` | `meta.attribute`, `entity.other.attribute-name` |
| `syntax.punctuation` | `punctuation`, `keyword.operator` |
| `syntax.heading` | `markup.heading` |
| `syntax.strong` | `markup.bold` |
| `syntax.emphasis` | `markup.italic` |
| `syntax.link` | `markup.underline.link` |
| `syntax.code` | `markup.inline.raw` |
| `syntax.semantic.package` | `entity.name.namespace` |
| `syntax.semantic.type` | `entity.name.type`, `entity.name.class` |
| `syntax.semantic.builtinType` | `support.type.primitive`, `support.type` |
| `syntax.semantic.call` | `meta.function-call entity.name.function`, `meta.function-call support.function` |
| `syntax.property` | `variable.other.property`, `variable.other.object.property`, `support.variable.property` |
| `syntax.globalVariable` | `support.variable`, `variable.other.global` |

### Semantic token colours

| Zen key | Selectors, in order |
|---|---|
| `syntax.semantic.package` | `namespace` |
| `syntax.semantic.type` | `type`, `class`, `struct`, `interface`, `enum`, `typeParameter` |
| `syntax.semantic.builtinType` | `type.defaultLibrary`, `class.defaultLibrary`, `builtinType` |
| `syntax.semantic.call` | `function`, `method` |
| `syntax.property` | `property` |
| `syntax.globalVariable` | `variable.defaultLibrary` |
| `syntax.function` | `function.declaration`, `method.declaration`, `function`, `method` |
| `syntax.macro` | `macro` |
| `syntax.keyword` | `keyword` |
| `syntax.string` | `string` |
| `syntax.number` | `number` |
| `syntax.comment` | `comment` |
| `syntax.constant` | `enumMember`, `variable.readonly` |
| `syntax.attribute` | `decorator` |
| `syntax.punctuation` | `operator` |
| `syntax.lifetime` | `lifetime` |

<!-- vscode:end -->

## Clean room

The importer was written from VS Code's public documentation of colour themes (the theme colour
reference, `tokenColors` with TextMate scopes, and the semantic highlight guide) and from the
TextMate documentation of scope selectors ([ADR-0002](../adr/0002-open-source-clean-room-pure-rust.md)).
No published theme is copied: the test fixtures in `crates/platform/theme/tests/fixtures/vscode` are
synthetic files written for the tests.
