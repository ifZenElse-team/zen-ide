# Themes

A theme sets the colours of a Zen IDE: the window, the editor, the styles of code and the terminal.
The IDE comes with two built-in themes: **Zen Dark** (`zen-dark`), the default, and **Zen Light**
(`zen-light`), its light counterpart. Other
themes are files in the format below; each changes only the colours it names and takes the rest
from its parent, and in the end from the built-in theme of its appearance: Zen Dark for a dark
theme, Zen Light for a light one ([ADR-0045](adr/0045-themes.md)).

The code is the `zen-theme` crate (`crates/platform/theme`): `zen_theme::parse` reads a file,
`zen_theme::to_json` writes one, `Theme::resolve` fills in the inherited colours,
`zen_theme::zen_dark` and `zen_theme::zen_light` are the built-in themes, and
`zen_theme::unreadable` and `zen_theme::make_readable` check and mend [readability](#readability). VS Code colour themes are imported with
`zen_theme::VsCodeImporter` ([themes/import-vscode.md](themes/import-vscode.md)), and themes of
JetBrains IDEs by `zen-jb-import` ([themes/import-jetbrains.md](themes/import-jetbrains.md)).

## Where themes live and how to choose one

An IDE keeps its themes in `themes/` in its data folder, next to `settings.json` (on macOS
`~/Library/Application Support/<app id>/themes/`; `ZEN_DATA_DIR` moves the data folder). Every
`*.json` file there in the format below is a theme; the IDE reads the folder at start, again whenever
Settings opens, and on `ui.theme.reload`. A file that cannot be read is listed with its error and cannot be chosen, and
never stops the IDE. A file cannot take the id of a built-in theme, and of two files with one id
the first by file name wins; the other is listed with an error. A theme's `parent` may be another
file of the folder.

`zen_theme::ThemeStore` holds the built-in themes, the files of the folder and the id of the theme
in use; it reads and writes files only through the `ThemeFiles` trait, which the IDE implements
with `zen_workbench::themes::DiskFiles`. The IDE creates one store at start with every importer
(`zen_jb_import::importers()` and `VsCodeImporter`) and shares it with every window. The choice is
saved as `theme` in `settings.json` (left out for Zen Dark); a saved theme that is gone gives way to
the built-in theme, with a notice in the first window drawn. Changing the choice redraws every
window from the next frame on, without a restart.

**Sync with OS.** The theme can follow the system's appearance instead: while the system is dark the
store uses one theme (`dark`, Zen Dark unless chosen), while it is light another (`light`, Zen Light
unless chosen). Choosing a theme while following makes it the theme of its own appearance, and
turning it on keeps the theme in use for its appearance. The window reports the system's appearance
with every frame (`egui::Context::system_theme`) and `ThemeStore::set_system_dark` switches the
theme the moment the system does. The choice is saved as `themeFollowSystem`, `themeDark` and
`themeLight` in `settings.json` (the built-in themes left out); `theme` keeps the theme used when
not following.

| Command | Parameters | Result |
|---|---|---|
| `ui.theme.list` | none | `{current, themes: [{id, name, dark, source: "builtin" or "user", path?, error?}], appearance: {followSystem, systemDark, dark, light}}` |
| `ui.theme.select` | `{id}` | `{}`; uses the theme in every window and saves the choice; while following the system, it becomes the theme of its appearance |
| `ui.theme.followSystem` | `{enabled, dark?, light?}` | `{}`; follows the system's appearance or stops; `dark` and `light` name the theme for each |
| `ui.theme.import` | `{path, select?}` | `{id, name, mapped, unmapped, adjusted}`; see below |
| `ui.theme.reload` | none | `{}`; reads the folder again; a theme in use that is gone gives way to the built-in theme of its appearance |

`ui.theme.import` reads a theme file of one of these formats, found from its contents and name: a
theme of this format (it has `formatVersion`), a JetBrains colour scheme (`.icls` or `.xml`), a
JetBrains UI theme (`.theme.json` or `.json`, with the colour scheme it names when that lies in the
same folder) or a VS Code colour theme (`.json`, with the files it includes). The theme is written
to `themes/<id>.json`, where an id that is not a plain file name (letters, digits, `-`, `_` and `.`,
not starting with `.`) is replaced by one made from the theme's name; when the id is taken by a built-in theme or another file, `-2`, `-3` and so
on is added. `mapped` counts the source's entries that became keys, and `unmapped` names the rest.
A theme from another editor builds on Zen Light when it is light and on Zen Dark when it is dark,
and its text is then made readable: `adjusted` names the keys that changed
([Readability](#readability)). With `select: true` the new theme is used at once. Agents reach the
same through the MCP tools `list_themes`, `select_theme`, `follow_system_theme` and `import_theme`
([automation.md](automation.md)), which need no open project.

## How the IDE uses the theme

Every colour of the IDE's windows, editor and terminal comes from a `ResolvedTheme`. `zen-ui` turns
it into egui colours in one place, `theme_bridge` (`zen_ui::theme_bridge::color`, and `CodeStyle`
for the code styles, bold, italic, underline and strikethrough included). `IdePalette::from_theme`
builds the palette the views draw with, with a field for each key they use, and
`TerminalPalette::from_theme` the terminal's; `IdePalette::apply` gives egui's own widgets the
popup, selection and caret colours. `IdeApp::set_theme` switches every window, the welcome screen
and the open terminals to another theme from the next frame on. Colours computed from others, such
as the dimmed primary button, the toolbar's glow and dim terminal text, are computed from the
palette. With Zen Dark the IDE looks exactly as it did before themes: a test pins the palette
against the old constants, and a test of the sources (`crates/platform/ui/tests/theme_literals.rs`)
fails on a colour written in the code of `ide/` or the terminal, except the few it lists with their
reasons (the loader's artwork, white as an image tint or for measuring text, colours a program in
the terminal sends). The editor draws a wavy underline of a style as a straight one.

## The file

A theme file is UTF-8 JSON. Comments (`//` to the end of the line, and `/* ... */`) and trailing
commas are allowed, as in the settings files of most editors. The JSON Schema is
[themes/theme.schema.json](themes/theme.schema.json); an editor that knows it completes the keys and
checks the colours. [themes/zen-dark.json](themes/zen-dark.json) is Zen Dark written out in full,
and [themes/zen-light.json](themes/zen-light.json) Zen Light.

```jsonc
{
  "formatVersion": 1,
  "id": "dusk",
  "name": "Dusk",
  "dark": true,
  "parent": "zen-dark",
  "ui": {
    "accent": "#7A5CF0",
    "link": "#A08CF8",
  },
  "editor": {
    "caret": "#FFCC66",
  },
  "syntax": {
    "keyword": "#E09060",
    "comment": { "color": "#7A7E85", "italic": true },
  },
  "terminal": {
    "cursor": "#FFCC66",
    // ANSI colours: black, red, green, yellow, blue, magenta, cyan, white, then the bright ones.
    "ansi": ["#45484D", "#F0524F", null, null, null, null, null, null,
             null, null, null, null, null, null, null, null],
  },
}
```

| Field | Type | Meaning |
|---|---|---|
| `$schema` | string, optional | A link to the schema, for editors. The IDE ignores it. |
| `formatVersion` | integer | The version of the format. It is `1`; a file with another version is not read. |
| `id` | string, optional | The stable id other themes name as their `parent`. When it is left out, it is made from the name: lower-case letters and digits joined by `-` (`My Theme` becomes `my-theme`). |
| `name` | string | The name shown in the theme list. |
| `dark` | boolean | Whether the theme is dark. |
| `parent` | string, optional | The id of the theme this one changes. When it is left out, the parent is Zen Dark, or Zen Light for a theme with `"dark": false`. |
| `ui` | object, optional | Window colours: key to colour. |
| `editor` | object, optional | Editor colours: key to colour. |
| `syntax` | object, optional | Code styles: key to a colour or a style object. |
| `terminal` | object, optional | `background`, `foreground` and `cursor` colours, and `ansi`: an array of exactly 16 colours, where `null` keeps the parent's colour. |

### Colours

A colour is a string of hexadecimal digits, in either case: `#RGB` (each digit doubled, so `#FA0`
is `#FFAA00`), `#RRGGBB`, or `#RRGGBBAA` with an alpha channel, where `00` is transparent and `FF`
opaque. Alpha is not premultiplied. Written files use upper case and leave out an opaque alpha.

### Styles

A `syntax` key takes a colour, or an object for more than a colour:

| Field | Type | Meaning |
|---|---|---|
| `color` | colour | The text colour. |
| `background` | colour, optional | A background behind the text. |
| `bold` | boolean, optional | Bold text. |
| `italic` | boolean, optional | Italic text. |
| `underline` | `"line"` or `"wave"`, optional | A straight or a wavy underline. |
| `strikethrough` | boolean, optional | Struck-through text. |

A style replaces the parent's style for that key as a whole: `{"color": "#FFFFFF"}` is plain white,
even if the parent's style was italic.

## Inheritance

A theme names its `parent` by id. The parent may be a built-in theme or another theme file, and it
may have a parent of its own: the nearest theme that sets a key wins. Below the last parent is
always a built-in theme, so every key has a colour: Zen Light when the farthest theme of the chain
is light (`"dark": false`), Zen Dark otherwise. A light theme therefore needs to set only the
colours it changes.

Problems in the chain never stop a theme from being used; they are reported as warnings:

- a parent that is not found ends the chain there;
- a chain that comes back to a theme already in it is a cycle, and ends before the repeat;
- at most 8 parents are followed; the farther ones are ignored.

## Unknown keys

A key, field or section this version does not know is ignored and reported as a warning (such as
`ui.no.such.key` or `syntax.keyword.weight`), so a theme written for a newer version still loads. A
malformed file, a colour that cannot be read, a wrong `formatVersion` or an `ansi` array that is not
16 long is an error, reported with its line and column.

## Readability

`zen_theme::PAIRS` lists every pair of a text colour and the background the IDE draws it on, with
the least WCAG 2 contrast ratio it needs: 4.5 for body text (`ui.text.primary` on the window,
islands, popups, fields, hovers and selections; `editor.foreground` on the editor, the current line,
search matches and diff lines; `syntax.default`; `terminal.foreground`), 3.0 for coloured labels
and marks (secondary text, links, errors, the file status colours, code styles, the ANSI colours,
text on the accent and on the project badges) and 2.0 for line numbers. A colour with alpha is
measured as painted over its section's background. `zen_theme::unreadable` returns the pairs a
resolved theme fails; both built-in themes pass all of them, which their tests check.

When a theme from another editor is imported, `zen_theme::make_readable` mends the pairs it fails,
whether the file set those colours or they came from the base theme: on a surface (the window, the
islands, popups, text fields, the editor and the terminal) the text colour moves towards black on
a light surface or towards white on a dark one, keeping its hue, until it reads; on any other
background (a selection, a hover, a diff line, a badge) that background moves away from the text
instead, so one text colour reads on all of them. The import's `adjusted` lists the keys changed,
and Settings says how many. Themes written in this format are taken as they are.

## Keys

Every key, what it colours, and its colour in Zen Dark and in Zen Light. The ANSI colours are written as the `ansi`
array of the `terminal` section; the table lists them as `ansi.0` to `ansi.15`.

<!-- keys:begin -->

### Window colours (`ui`)

| Key | Meaning | Zen Dark | Zen Light |
|---|---|---|---|
| `window.background` | The window behind the panels: the main toolbar, the stripes and the status bar. | `#25282C` | `#EBECF0` |
| `island.background` | The raised panels: tool windows and the frame of the editor. | `#191A1D` | `#FFFFFF` |
| `text.primary` | Primary text. | `#DFE1E5` | `#1E1F22` |
| `text.secondary` | Secondary text: paths, hints, status bar items, inactive icons. | `#868A91` | `#6C707E` |
| `text.error` | Error messages in dialogs and settings. | `#E56E6E` | `#CC2E3E` |
| `text.warning` | Warnings in dialogs and settings, such as a shortcut conflict. | `#E5A04B` | `#9E6200` |
| `link` | Links and link-styled actions. | `#6CA4F8` | `#2F5EB8` |
| `accent` | The accent: primary buttons, focused fields, the active stripe button. | `#3574F0` | `#3574F0` |
| `accent.foreground` | Text on the accent colour, such as a primary button's label. | `#FFFFFF` | `#FFFFFF` |
| `list.hover` | A hovered row or button in a panel. | `#2C2E33` | `#EDEEF1` |
| `list.selection` | A selected row in a list or tree. | `#2E436E` | `#D4E2FF` |
| `searchHighlight` | The background of the part of a label that matches a search, as in the Settings dialog. | `#2F4C3A` | `#CCE8CF` |
| `popup.background` | Popups, menus and dialogs, which float a step lighter than the window. | `#2B2D30` | `#FFFFFF` |
| `popup.border` | The outline of popups, menus and dialogs. | `#43454A` | `#C9CCD6` |
| `popup.hover` | A hovered item in a popup or menu. | `#3A3D43` | `#DFE7F8` |
| `popup.shadow` | The shadow under popups and dialogs. | `#0000006E` | `#00000038` |
| `popup.code.background` | The background of code in documentation popups. | `#80808030` | `#80808020` |
| `textField.background` | The inside of a text field in a dialog. | `#1E1F22` | `#FFFFFF` |
| `textField.border` | The outline of a text field that has no focus. | `#4E5157` | `#C9CCD6` |
| `textField.selection` | Selected text in a text field. | `#214283` | `#B4D0FF` |
| `textField.caret` | The caret in a text field. | `#CED0D6` | `#1E1F22` |
| `button.secondary.border` | The outline of a secondary button. | `#4E5157` | `#C9CCD6` |
| `tab.border` | Borders of tabs, tool bars and panels. | `#4E5157` | `#DFE1E5` |
| `tab.active.background` | The fill of the active editor tab. | `#273555` | `#E5ECFB` |
| `tab.active.border` | The outline of the active editor tab. | `#354C85` | `#A9C1F2` |
| `tab.modified` | The name of an editor tab with unsaved changes. | `#6CA4F8` | `#2F5EB8` |
| `tree.excluded.text` | Excluded folders, such as build output, in the project tree; also invalid input. | `#D88A3C` | `#A85700` |
| `tree.excluded.background` | The row of an excluded folder in the project tree. | `#291F1C` | `#FDF0E4` |
| `tree.testRow` | The row of a test file in the project tree. | `#1E261D` | `#EEF7EE` |
| `vcs.modified` | A file changed since the last commit: its name in the project tree, the tabs and the change lists. | `#6CA4F8` | `#2F5EB8` |
| `vcs.added` | A new file added to version control. | `#73BD79` | `#2F7D32` |
| `vcs.deleted` | A deleted file. | `#6F737A` | `#6C707E` |
| `vcs.unversioned` | A file not known to version control. | `#D1675A` | `#B54434` |
| `vcs.ignored` | A file version control ignores. | `#A8A34F` | `#7A7514` |
| `vcs.conflict` | A file with merge conflicts. | `#E55C5C` | `#CC1F1F` |
| `vcs.incoming` | The count of commits to pull, in the branches popup. | `#6CA4F8` | `#2F5EB8` |
| `vcs.outgoing` | The count of commits to push, in the branches popup. | `#73BD79` | `#2F7D32` |
| `vcs.currentBranch` | The label icon of the current branch, in the branches popup. | `#E8B94F` | `#C88A00` |
| `vcs.graph.0` | The first lane colour of the log graph. | `#5F9EE8` | `#3D7FD9` |
| `vcs.graph.1` | The second lane colour of the log graph. | `#6AB86E` | `#3E9B45` |
| `vcs.graph.2` | The third lane colour of the log graph. | `#D98C4A` | `#D97A2B` |
| `vcs.graph.3` | The fourth lane colour of the log graph. | `#B07CD8` | `#9654CF` |
| `vcs.graph.4` | The fifth lane colour of the log graph. | `#3FB8B0` | `#1F9C93` |
| `vcs.graph.5` | The sixth lane colour of the log graph. | `#D86F7E` | `#D2506A` |
| `banner.background` | Editor notifications, the banner above the editor. | `#25324D` | `#E7EEFC` |
| `icon.run` | The Run button and other run icons. | `#5FB865` | `#3E9B45` |
| `stripe.active.icon` | The icon of the active tool window stripe button. | `#FFFFFF` | `#FFFFFF` |
| `image.checker.light` | The light squares of the board behind transparent images. | `#4A4D52` | `#FFFFFF` |
| `image.checker.dark` | The dark squares of the board behind transparent images. | `#3A3C40` | `#E6E7EA` |
| `project.0` | The first project colour: a project's badge and the glow of the main toolbar. | `#4A7FD6` | `#4A7FD6` |
| `project.1` | The second project colour. | `#3E8E7E` | `#3E8E7E` |
| `project.2` | The third project colour. | `#C06E45` | `#C06E45` |
| `project.3` | The fourth project colour. | `#965EC2` | `#965EC2` |
| `project.4` | The fifth project colour. | `#3F9AB0` | `#3F8AA0` |
| `project.5` | The sixth project colour. | `#B85A8A` | `#B85A8A` |
| `project.foreground` | The initials on a project's badge. | `#FFFFFF` | `#FFFFFF` |

### Editor colours (`editor`)

| Key | Meaning | Zen Dark | Zen Light |
|---|---|---|---|
| `background` | The editor's background. | `#191A1D` | `#FFFFFF` |
| `foreground` | Text that is not highlighted, such as a folded region's tail or a preview. | `#BCBEC4` | `#080808` |
| `lineNumber` | Line numbers in the gutter. | `#4B5059` | `#9A9EAB` |
| `currentLine` | The line with the caret. | `#1F2024` | `#F5F8FE` |
| `caret` | The caret. | `#CED0D6` | `#000000` |
| `selection` | Selected text. | `#2E436E` | `#A6D2FF` |
| `search.match` | A search match. | `#2F4C3A` | `#F5E7A1` |
| `search.mark` | The marks of search matches by the scroll bar. | `#5F8C63` | `#D9B530` |
| `fold.marker` | The background of a folded region's mark. | `#3B3E44` | `#E4E5E8` |
| `fold.arrow` | The fold arrows in the gutter. | `#707379` | `#8C8F99` |
| `fold.text` | The text of a folded region's mark. | `#868A91` | `#5E626E` |
| `inlayHint.background` | The background of an inlay hint, such as a parameter name before an argument. | `#2B2D31` | `#EDEEF1` |
| `inlayHint.foreground` | The text of an inlay hint. | `#8C9097` | `#5E626E` |
| `gutter.separator` | The thin line between the gutter and the text. | `#323438` | `#EBECF0` |
| `methodSeparator` | The thin line above each top-level declaration while Show method separators is on. | `#43454A` | `#D3D5DB` |
| `stickyLines.background` | The background of the sticky lines over the top of the editor. | `#1D1E21` | `#F7F8FA` |
| `stickyLines.border` | The line under the sticky lines. | `#323438` | `#E1E3E8` |
| `symbolHighlight` | The background of the name at the caret and of its other occurrences in the file, and of the places the symbol is read. | `#383B39` | `#E4E4FF` |
| `symbolHighlight.write` | The background of the places the symbol at the caret is written: declared, assigned or changed. | `#4A363A` | `#FCE2E4` |
| `bracket.matched.background` | The background of the bracket next to the caret and of the bracket that matches it. | `#3B514D` | `#99CCFF` |
| `bracket.matched.foreground` | The bracket next to the caret and the bracket that matches it. | `#FFEF28` | `#080808` |
| `bracket.unmatched.background` | The background of the bracket next to the caret when nothing matches it. | `#5E3838` | `#F7C6C6` |
| `bracket.unmatched.foreground` | The bracket next to the caret when nothing matches it. | `#BCBEC4` | `#080808` |
| `diagnostic.error` | An error. | `#F75464` | `#D72B3F` |
| `diagnostic.warning` | A warning. | `#E0B04E` | `#B57A00` |
| `diagnostic.info` | Information. | `#6F9FE8` | `#3574F0` |
| `diagnostic.hint` | A hint, or a problem of unknown severity. | `#868A91` | `#8C8F99` |
| `diagnostic.typo` | A misspelt word, from the spelling inspection: its wavy line and its mark. | `#5FA36A` | `#4E9A5B` |
| `completion.function` | The mark of a function, method or constructor in the completion list. | `#C08BF5` | `#8A4FD6` |
| `completion.variable` | The mark of a variable or value in the completion list. | `#6FB3D9` | `#1D7FAD` |
| `completion.field` | The mark of a field or property in the completion list. | `#E0B04E` | `#A86E00` |
| `completion.constant` | The mark of a constant or enum member in the completion list. | `#9AC67A` | `#3E8A3E` |
| `completion.type` | The mark of a struct, class or enum in the completion list. | `#5FA8F0` | `#2E6FD1` |
| `completion.interface` | The mark of an interface or type parameter in the completion list. | `#5FC8B0` | `#1F8C77` |
| `completion.module` | The mark of a module or package in the completion list. | `#D08C6A` | `#B25C33` |
| `completion.keyword` | The mark of a keyword in the completion list. | `#CF8E6D` | `#0033B3` |
| `completion.snippet` | The mark of a snippet in the completion list. | `#A0A4AC` | `#6C707E` |
| `completion.other` | The mark of any other kind of item in the completion list. | `#A0A4AC` | `#6C707E` |
| `gutter.added` | The change marker in the gutter of lines added since the last commit. | `#549159` | `#8FCB94` |
| `gutter.modified` | The change marker in the gutter of lines modified since the last commit. | `#375FAD` | `#8EAEEB` |
| `gutter.deleted` | The change marker in the gutter where lines were deleted since the last commit. | `#868A91` | `#A8ABB3` |
| `diff.inserted` | The background of an inserted line in the diff viewer. | `#1F3526` | `#DDF2DE` |
| `diff.deleted` | The background of a deleted line in the diff viewer. | `#34363A` | `#EAEBEE` |
| `diff.modified` | The background of a modified line in the diff viewer. | `#1E2E4B` | `#E3ECFC` |
| `diff.insertedWord` | The background of the changed words of an inserted line in the diff viewer. | `#2C5437` | `#B6E2B9` |
| `diff.deletedWord` | The background of the changed words of a deleted line in the diff viewer. | `#4A4D52` | `#D0D3D9` |
| `diff.modifiedWord` | The background of the changed words of a modified line in the diff viewer. | `#2B4575` | `#C2D5F8` |
| `diff.filler` | The filler rows of the diff viewer across from lines the other side does not have. | `#222327` | `#F4F5F7` |
| `breakpoint` | A line breakpoint in the gutter, drawn over the line number. | `#E55C5C` | `#E55C5C` |
| `breakpoint.disabled` | The outline of a disabled line breakpoint in the gutter. | `#E56E6E` | `#E56E6E` |
| `execution.line` | The background of the line where the debugger stopped. | `#273555` | `#DCE6FA` |
| `execution.arrow` | The arrow in the gutter at the line where the debugger stopped. | `#6CA4F8` | `#3574F0` |
| `annotation.recent` | Blame annotations of the newest commits in the gutter; older ones fade towards annotation.old. | `#E8B94F` | `#9E6200` |
| `annotation.old` | Blame annotations of the oldest commits in the gutter. | `#5A5E66` | `#A8ABB3` |
| `coverage.covered` | The coverage mark in the gutter of a line the tests ran, and its line in the HTML report. | `#4E8A52` | `#6CBF73` |
| `coverage.partly` | The coverage mark in the gutter of a line the tests ran only partly: some of its parts or branches did not run. | `#B08A2E` | `#E8C25A` |
| `coverage.notCovered` | The coverage mark in the gutter of a line the tests did not run. | `#A84A48` | `#E87E7A` |
| `unused` | Unused code, such as an unused requirement or import: drawn in this colour instead of underlined. | `#6F737A` | `#8C8C8C` |
| `whitespace` | The dots of spaces and the marks of tabs while whitespaces are shown. | `#4B4E54` | `#BEC1C8` |
| `indentGuide` | The thin vertical lines at each indent level. | `#2E3035` | `#EBECF0` |
| `indentGuide.selected` | The indent guide of the block the caret is in. | `#50535A` | `#C4C7CF` |
| `rightMargin` | The vertical line at the right margin and the visual guides of the code style. | `#393B40` | `#E1E3E8` |

### Code styles (`syntax`)

| Key | Meaning | Zen Dark | Zen Light |
|---|---|---|---|
| `keyword` | A keyword, such as `fn` or `func`. | `#CF8E6D` | `#0033B3` |
| `string` | A string or character literal. | `#6AAB73` | `#067D17` |
| `number` | A number literal. | `#2AACB8` | `#1750EB` |
| `comment` | A comment. | `#7A7E85` | `#7F7F7F` |
| `docComment` | A documentation comment. | `#5F826B italic` | `#5A7763 italic` |
| `docComment.name` | The name a documentation comment starts with, such as `Open` in `// Open opens a file`. | `#A1A1A1` | `#5E626E` |
| `macro` | A macro invocation, such as `println!`. | `#56A8F5` | `#1F5FA8` |
| `type` | A type or trait name, as the highlighter sees it. | `#BCBEC4` | `#080808` |
| `function` | A function name in a definition or call. | `#56A8F5` | `#00627A` |
| `lifetime` | A lifetime or label, such as `'a`. | `#20999D` | `#208084` |
| `constant` | A constant or static. | `#C77DBB italic` | `#871094 italic` |
| `attribute` | An attribute, such as `#[derive(Debug)]`. | `#B3AE60` | `#806E0A` |
| `punctuation` | Punctuation and operators. | `#BCBEC4` | `#080808` |
| `heading` | A heading in a markup language, such as `# Title` in Markdown. | `#C87DBB italic` | `#871094 italic` |
| `strong` | Strong text in a markup language. | `#DFE1E5` | `#000000` |
| `emphasis` | Emphasised text in a markup language. | `#BCBEC4 italic` | `#080808 italic` |
| `link` | A link's text in a markup language. | `#56A8F5` | `#2F5EB8` |
| `code` | Code inside a markup language: inline code and code blocks. | `#6AAB73` | `#067D17` |
| `todo` | TODO text in a comment, as the patterns of Settings → Editor → TODO find it. | `#8FB8E8` | `#1F5FA8` |
| `semantic.package` | A package name, from the language server. | `#B2BF85` | `#4D6B1F` |
| `semantic.type` | A type that is not built in, from the language server. | `#7EADBB` | `#007680` |
| `semantic.builtinType` | A built-in type, from the language server. | `#CF8E6D` | `#0033B3` |
| `semantic.call` | A call of a function or a method, from the language server; declarations keep `function`. | `#AD9E7D` | `#7A5B12` |
| `property` | A property or field where it is declared, such as `name` in `{ name: string }`. | `#C77DBB` | `#871094` |
| `globalVariable` | A global variable, such as `console`, `window` or a module-level variable. | `#C77DBB italic` | `#871094 italic` |
| `navigationLink` | The name under the pointer while ⌘ (Ctrl) is held, which a click goes to. | `#6CA4F8 underline` | `#2F5EB8 underline` |
| `default` | Code that no other key covers. | `#BCBEC4` | `#080808` |

### Terminal colours (`terminal`)

| Key | Meaning | Zen Dark | Zen Light |
|---|---|---|---|
| `background` | The default background. | `#1F2023` | `#FFFFFF` |
| `foreground` | The default text colour. | `#DFE1E5` | `#1E1F22` |
| `cursor` | The block cursor. | `#CED0D6` | `#1E1F22` |
| `selection` | Selected text. | `#2E436E` | `#A6D2FF` |
| `ansi.0` | ANSI black. | `#45484D` | `#2B2D30` |
| `ansi.1` | ANSI red. | `#E58A86` | `#C4372F` |
| `ansi.2` | ANSI green. | `#93C49A` | `#2F7D32` |
| `ansi.3` | ANSI yellow. | `#E3C47E` | `#8F6400` |
| `ansi.4` | ANSI blue. | `#8AAFE6` | `#2D5FBF` |
| `ansi.5` | ANSI magenta. | `#CDA2DC` | `#9B3FA6` |
| `ansi.6` | ANSI cyan. | `#86C7C8` | `#137A84` |
| `ansi.7` | ANSI white. | `#C4C6CC` | `#9EA1AA` |
| `ansi.8` | ANSI bright black. | `#6F737A` | `#6C707E` |
| `ansi.9` | ANSI bright red. | `#F2A7A3` | `#D04A41` |
| `ansi.10` | ANSI bright green. | `#ADD5B2` | `#3B8C40` |
| `ansi.11` | ANSI bright yellow. | `#EED69C` | `#A07200` |
| `ansi.12` | ANSI bright blue. | `#A9C6F2` | `#3F75D6` |
| `ansi.13` | ANSI bright magenta. | `#DDBCE8` | `#B04FBA` |
| `ansi.14` | ANSI bright cyan. | `#A6DADB` | `#1A8A94` |
| `ansi.15` | ANSI bright white. | `#F2F3F5` | `#C4C7CF` |

<!-- keys:end -->
