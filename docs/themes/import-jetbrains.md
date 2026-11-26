# Importing JetBrains themes

The `zen-jb-import` crate (`crates/platform/jb-import`, layer L5) turns the theme files of JetBrains
IDEs into Zen IDE [themes](../themes.md): colour schemes (`.icls`, and colour scheme `.xml` files)
and UI themes (`.theme.json`). Both importers implement `zen_theme::ThemeImporter`
([ADR-0045](../adr/0045-themes.md)); `zen_jb_import::importers()` returns them for the IDE to
register. Choosing a file to import in the IDE comes later.

| Importer | Files | Detected by |
|---|---|---|
| `IclsImporter` | `.icls`, `.xml` | The root element is `<scheme>`. |
| `ThemeJsonImporter` | `.theme.json` | A JSON object (comments and trailing commas allowed) with a `ui` object or an `editorScheme` string, and no `tokenColors` or `semanticTokenColors`, which belong to VS Code themes. |

`detects(text)` looks at the text only; `detects_path(path, text)` also checks the file's extension.

An imported theme is sparse: it sets only the keys the file gave a colour, and its parent is the
built-in theme of its appearance, so it resolves to a complete theme without warnings. Its id is
made from its name as for any theme file. A light scheme (one whose `parent_scheme` is not Darcula
or another dark scheme, or a UI theme with `"dark": false`) is marked `dark: false` and builds on
Zen Light (`zen-light`); a dark one builds on Zen Dark (`zen-dark`). Code styles without a
foreground take the scheme's text colour, or the base theme's. When the theme is imported into the
IDE's themes folder, text too faint on its background is made readable
([themes.md](../themes.md#readability)).

## What is not carried over

The `ImportReport` of an import counts the source entries that became keys (`mapped`) and lists the
others by their name in the file (`unmapped`): every colour or attribute without a row below, every
attribute whose `baseAttributes` chain does not end in this file (or is a cycle), every value that is
not a colour, and every `ui` key that no row reads. Nothing in the list is an error: the theme loads
without it. A file that is not well-formed XML or JSON, or whose root is not a colour scheme or a
JSON object, is an `ImportError`.

## Colour schemes

A colour scheme is XML:

```xml
<scheme name="My Scheme" version="142" parent_scheme="Darcula">
  <colors>
    <option name="CARET_COLOR" value="ffcc66" />
  </colors>
  <attributes>
    <option name="DEFAULT_KEYWORD">
      <value>
        <option name="FOREGROUND" value="e09060" />
        <option name="FONT_TYPE" value="1" />
      </value>
    </option>
    <option name="DEFAULT_BRACES" baseAttributes="DEFAULT_OPERATION_SIGN" />
  </attributes>
</scheme>
```

- **Colours** are hexadecimal digits without `#`, `RRGGBB` (or `RRGGBBAA`). A shorter value is
  padded with zeros on the left, so `ff` is `#0000FF` and `0` is black. An empty value means "not
  set".
- **Attributes** hold `FOREGROUND`, `BACKGROUND`, `FONT_TYPE` (`1` bold, `2` italic, `3` both),
  `EFFECT_TYPE` with `EFFECT_COLOR`, and `ERROR_STRIPE_COLOR`. An effect is drawn only when it has
  a colour: `EFFECT_TYPE` `1` (underscored), `4` (bold underscored) and `5` (dotted) become a
  straight underline, `2` a wavy underline, `3` strikethrough; `0` (a box) and other effects are
  dropped.
- **`baseAttributes`** names another attribute of the same file to take the value from when the
  option has no `<value>` of its own; chains are followed up to 16 links.
- **`parent_scheme`** decides only `dark`: Darcula or a name with "dark" in it is dark, anything
  else light. Without one, the scheme is dark unless the background of `TEXT` is light.
- When several sources map to one key, the first row of the table that the file has wins.

### Colours (`<colors>`)

| Scheme colour | Zen key |
|---|---|
| `CARET_COLOR` | `editor.caret` |
| `CARET_ROW_COLOR` | `editor.currentLine` |
| `SELECTION_BACKGROUND` | `editor.selection` |
| `LINE_NUMBERS_COLOR` | `editor.lineNumber` |
| `STICKY_LINES_BACKGROUND` | `editor.stickyLines.background` |
| `METHOD_SEPARATORS_COLOR` | `editor.methodSeparator` |
| `ADDED_LINES_COLOR` | `editor.gutter.added` |
| `MODIFIED_LINES_COLOR` | `editor.gutter.modified` |
| `DELETED_LINES_COLOR` | `editor.gutter.deleted` |
| `ANNOTATIONS_LAST_COMMIT_COLOR` | `editor.annotation.recent` |
| `ANNOTATIONS_COLOR` | `editor.annotation.old` |
| `WHITESPACES` | `editor.whitespace` |
| `INDENT_GUIDE` | `editor.indentGuide` |
| `SELECTED_INDENT_GUIDE` | `editor.indentGuide.selected` |
| `RIGHT_MARGIN_COLOR` | `editor.rightMargin` |
| `FILESTATUS_MODIFIED` | `ui.vcs.modified` |
| `FILESTATUS_ADDED` | `ui.vcs.added` |
| `FILESTATUS_DELETED` | `ui.vcs.deleted` |
| `FILESTATUS_UNKNOWN` | `ui.vcs.unversioned` |
| `FILESTATUS_IDEA_FILESTATUS_IGNORED` | `ui.vcs.ignored` |
| `FILESTATUS_IDEA_FILESTATUS_MERGED_WITH_CONFLICTS` | `ui.vcs.conflict` |

### Attributes to editor colours

| Scheme attribute | Field | Zen key |
|---|---|---|
| `TEXT` | foreground | `editor.foreground` |
| `TEXT` | background | `editor.background` |
| `TEXT_SEARCH_RESULT_ATTRIBUTES` | background | `editor.search.match` |
| `TEXT_SEARCH_RESULT_ATTRIBUTES` | error stripe | `editor.search.mark` |
| `FOLDED_TEXT_ATTRIBUTES` | background | `editor.fold.marker` |
| `FOLDED_TEXT_ATTRIBUTES` | foreground | `editor.fold.text` |
| `INLAY_DEFAULT` | background | `editor.inlayHint.background` |
| `INLAY_DEFAULT` | foreground | `editor.inlayHint.foreground` |
| `INLINE_PARAMETER_HINT` | background, when `INLAY_DEFAULT` has none | `editor.inlayHint.background` |
| `INLINE_PARAMETER_HINT` | foreground, when `INLAY_DEFAULT` has none | `editor.inlayHint.foreground` |
| `IDENTIFIER_UNDER_CARET_ATTRIBUTES` | background | `editor.symbolHighlight` |
| `WRITE_IDENTIFIER_UNDER_CARET_ATTRIBUTES` | background | `editor.symbolHighlight.write` |
| `MATCHED_BRACE_ATTRIBUTES` | background | `editor.bracket.matched.background` |
| `MATCHED_BRACE_ATTRIBUTES` | foreground | `editor.bracket.matched.foreground` |
| `UNMATCHED_BRACE_ATTRIBUTES` | background | `editor.bracket.unmatched.background` |
| `UNMATCHED_BRACE_ATTRIBUTES` | foreground | `editor.bracket.unmatched.foreground` |
| `ERRORS_ATTRIBUTES` | effect, else foreground, else error stripe | `editor.diagnostic.error` |
| `WARNING_ATTRIBUTES` | effect, else foreground, else error stripe | `editor.diagnostic.warning` |
| `INFO_ATTRIBUTES` | effect, else foreground, else error stripe | `editor.diagnostic.info` |
| `WEAK_WARNING_ATTRIBUTES` | effect, else foreground, else error stripe | `editor.diagnostic.hint` |
| `TYPO` | effect, else foreground, else error stripe | `editor.diagnostic.typo` |
| `NOT_USED_ELEMENT_ATTRIBUTES` | foreground | `editor.unused` |
| `DIFF_INSERTED` | background | `editor.diff.inserted` |
| `DIFF_DELETED` | background | `editor.diff.deleted` |
| `DIFF_MODIFIED` | background | `editor.diff.modified` |

### Attributes to code styles

The whole attribute becomes the style: foreground, background, bold, italic, underline and
strikethrough. An attribute without a foreground takes the foreground of `TEXT`, or the base theme's
colour of the key (Zen Dark's or Zen Light's) when the scheme has no `TEXT` foreground. `syntax.default` takes no background,
since the background of `TEXT` is the editor's.

| Scheme attribute | Zen key |
|---|---|
| `TEXT` | `syntax.default` |
| `DEFAULT_KEYWORD` | `syntax.keyword` |
| `DEFAULT_STRING` | `syntax.string` |
| `DEFAULT_NUMBER` | `syntax.number` |
| `DEFAULT_LINE_COMMENT`, then `DEFAULT_BLOCK_COMMENT` | `syntax.comment` |
| `DEFAULT_DOC_COMMENT` | `syntax.docComment` |
| `DEFAULT_FUNCTION_DECLARATION` | `syntax.function` |
| `DEFAULT_FUNCTION_CALL` | `syntax.semantic.call` |
| `CTRL_CLICKABLE` | `syntax.navigationLink` |
| `DEFAULT_CLASS_NAME`, then `DEFAULT_INTERFACE_NAME` | `syntax.type` |
| `DEFAULT_CONSTANT` | `syntax.constant` |
| `DEFAULT_METADATA` | `syntax.attribute` |
| `DEFAULT_LABEL` | `syntax.lifetime` |
| `DEFAULT_OPERATION_SIGN`, then `DEFAULT_BRACES`, `DEFAULT_PARENTHS`, `DEFAULT_BRACKETS`, `DEFAULT_COMMA`, `DEFAULT_SEMICOLON`, `DEFAULT_DOT` | `syntax.punctuation` |

Language-specific attributes (of Go, for example) are not mapped yet; they are listed as unmapped.
Scheme options outside `<colors>` and `<attributes>`, such as `LINE_SPACING`, are listed too.

## UI themes

A UI theme is JSON:

```jsonc
{
  "name": "My Theme",
  "dark": true,
  "author": "Me",
  "editorScheme": "/themes/MyScheme.xml",
  "colors": { "accent": "#7A5CF0", "accentLight": "accent" },
  "ui": {
    "*": { "background": "#1E1F22" },
    "Button": { "default": { "startBackground": "accent" } },
    "Link.activeForeground": "accentLight"
  }
}
```

- **`ui`** objects are flattened with `.`: `{"Button": {"default": {"startBackground": ...}}}` is
  the key `Button.default.startBackground`.
- **`"*"`** is a wildcard: `*.background` applies to every key that ends in `.background` and is
  not set itself. The longest matching wildcard wins; an exact key always wins over a wildcard.
- **Values** are `#RRGGBB` or `#RRGGBBAA` colours, or names from `colors`; a name may point to
  another name, up to 8 steps. Values that are not colours (numbers, sizes, booleans) are listed
  as unmapped.
- **`name`** and **`dark`** become the theme's name and `dark` flag; `author` is ignored. Other top
  level keys, such as `icons`, are listed as unmapped.
- **`editorScheme`** is a path inside the plug-in that shipped the theme. `ThemeJsonImporter::import_file`
  (and `scheme_path`) looks for that file name in the folder of the `.theme.json`, then for the
  same name with `.icls` or `.xml`; when it is found, it is imported too and merged: the window
  colours come from the UI theme, the editor colours and code styles from the scheme, and both
  reports are added up. When the scheme is not found, or the text was imported without a path
  (`import`), `editorScheme` is listed as unmapped. `import_with_scheme(text, scheme)` takes both
  texts directly.

| UI theme key (first one found) | Zen key |
|---|---|
| `Panel.background` (or `*.background`) | `ui.window.background` |
| `ToolWindow.background`, `Editor.background` | `ui.island.background` |
| `Label.foreground` (or `*.foreground`) | `ui.text.primary` |
| `Component.infoForeground`, `Label.infoForeground` | `ui.text.secondary` |
| `Label.errorForeground` | `ui.text.error` |
| `Link.activeForeground` | `ui.link` |
| `Button.default.startBackground` | `ui.accent` |
| `Button.default.foreground` | `ui.accent.foreground` |
| `List.hoverBackground`, `Tree.hoverBackground` | `ui.list.hover` |
| `Tree.selectionBackground`, `List.selectionBackground` | `ui.list.selection` |
| `Popup.background` | `ui.popup.background` |
| `Popup.borderColor` | `ui.popup.border` |
| `MenuItem.selectionBackground` | `ui.popup.hover` |
| `TextField.background` | `ui.textField.background` |
| `Component.borderColor` | `ui.textField.border` |
| `TextField.selectionBackground` | `ui.textField.selection` |
| `TextField.caretForeground` | `ui.textField.caret` |
| `Button.startBorderColor` | `ui.button.secondary.border` |
| `Borders.color` | `ui.tab.border` |
| `EditorTabs.underlinedTabBackground` | `ui.tab.active.background` |
| `EditorTabs.underlineColor` | `ui.tab.active.border` |

Every key of the table is also reached through a matching wildcard, so `*.selectionBackground` sets
the list, popup and text field selections at once.

## Clean-room

The importers and their mapping tables (`crates/platform/jb-import/src/icls.rs` for colour schemes,
`theme_json.rs` for UI themes) are written from the public documentation of the two formats (the
IntelliJ Platform SDK's pages on colour schemes and UI themes) and from files we wrote ourselves. No colour scheme,
UI theme or source file of a JetBrains product is copied into the repository; the test fixtures in
`crates/platform/jb-import/tests/fixtures/` are synthetic, with our own colours
([ADR-0002](../adr/0002-open-source-clean-room-pure-rust.md), [ADR-0011](../adr/0011-plugins-and-jetbrains-compatibility.md)).
