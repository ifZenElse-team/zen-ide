# ADR-0053: Fonts, UI scale and code style per language

- **Status:** Proposed
- **Date:** 2026-10-18

## Context

The requirement: the pages of the Settings dialog (Appearance, Editor, Go) that people use first:
the UI font size on Appearance & Behavior, the editor's font
family, size and line spacing on Editor → Font, the indentation of each language on Editor → Code
Style, the Go page, and a search over all settings. Until now the dialog was one file with a fixed
list of sections addressed by index, the UI and the editor drew at a fixed 13 points with the bundled
fonts (Inter and a monospaced editor font), and every language was indented by the formatter alone.

Several people build these pages at once, so the dialog needs a structure where each page is its own
file and adding a section does not touch the others. The code style also matters outside the UI:
project files (`.idea/codeStyles`, later `.editorconfig`) set it per project, so its type cannot live
in the UI crate.

## Decision

1. **The dialog is a registry.** `zen-ui`'s `ide/settings/page.rs` lists the section tree as
   `SectionDef { key, level, page: PageId, search_keys }`; the dialog draws the tree from it and
   chooses the page with a `match` on `PageId`. Each page lives in its own file under
   `ide/settings/`, and new pages draw with the shared helpers of `ide/settings/widgets.rs` (group
   title, label, check box, number field, path field), which mark the part of a label
   that matches the search with the theme key `ui.searchHighlight`. A page that is not built yet says
   "In development". Each page has its own Fluent file (`settings-<page>.ftl`).
2. **The UI font size zooms the whole UI.** Appearance & Behavior → UI font size (`uiFontSize`, 13 by
   default) sets egui's zoom factor to `size / 13`, so every metric of the UI scales with the text.
   The editor's font size is independent of it.
3. **Editor fonts are the bundled editor font and the monospaced fonts installed on the machine.**
   The installed fonts are found with `fontdb` (pure Rust, MIT); nothing new is bundled.
   `editorFont` holds the family (`None` is the bundled font), the size (13) and the line spacing (1.2).
   `fontdb` 0.24 is used without its default features (`std` and `fs` only): no memory-mapped files
   (its only `unsafe`) and no fontconfig parsing; it reads the font files of the system font folders
   and parses them in Rust. It is a dependency of `zen-ui` on desktop targets only. The installed
   fonts are read once, on a background thread, the first time the Font page's list opens or a
   chosen family has to be loaded; the list is the faces `fontdb` marks as monospaced, by family
   name, without hidden (`.`) system families and without the bundled font. The chosen
   family's regular, bold and italic faces become the egui families `zen-editor`,
   `zen-editor-bold` and `zen-editor-italic`, each followed by the matching face of the bundled font and
   egui's fallbacks; a face the family lacks is the bundled font's. The Font page's preview has its own
   families (`zen-editor-preview…`), so the draft is shown before it is applied. Agents list the
   fonts with the command `ui.fonts.list`, registered by `zen-ui`, since the layers below it cannot
   read fonts.
   The editor's metrics come from these settings every frame: the line height is
   `round(24 × size / 13 × spacing / 1.2)` pixels (24 at the defaults), and the font size and line
   height are divided by the UI zoom so that the editor's text keeps its size when the UI is scaled.
4. **The code style of each language** is `zen_project::settings::CodeStyle` (`useTabs`, `tabSize`,
   `indent`) in layer L1, so that project settings and importers share it. `IdeSettings.codeStyle`
   maps a language id to a `CodeStyle`; a language without an entry takes the built-in table
   `CodeStyle::BUILT_IN` (Go: tabs, 4; JSON, YAML and TOML: spaces, 2; Markdown and any other
   language: spaces, 4). The editor and the formatter read it through session settings keys written
   by `IdeSettings::apply_to_session`, as the other settings are (ADR-0007).
   The keys are `editor.codeStyle.<language>.useTabs`, `.tabSize` and `.indent`, written only for the
   languages the user changed; `zen_workbench::code_style` names a file's language by its extension
   with the ids language servers use (`go`, `json`, `yaml`, `toml`, `markdown`, …), and a file of a
   language without an entry keeps tabs when it is already indented with them (as before), otherwise
   takes the built-in style. Tab, Enter and indenting insert the style's unit, Reformat Code sends
   it as `tabSize` and `insertSpaces`, and the editor draws tabs `tabSize` columns wide.
5. **`.editorconfig` comes later**, as a project source of the same `CodeStyle`.
6. **The settings search looks through translated texts**: the titles of the sections and the
   messages each `SectionDef` lists in `search_keys`, in the language in use, so a Russian user
   searches in Russian.

## Consequences

- A new settings page is a `PageId`, a row in `SECTIONS`, a file and a Fluent file; it does not
  conflict with work on the other pages.
- Settings files written before these fields load unchanged: every new field has a default.
- `fontdb` becomes a dependency of `zen-ui` when Editor → Font is built.
- The search covers only what the pages register in `search_keys`; a page that forgets a message is
  not found by it.

## Alternatives considered

- **UI font size as a font size only:** egui's spacing, icons and the other metrics of the UI
  would stay at their 13-point sizes, and the layout would break at other sizes.
- **Bundling more editor fonts:** a larger download, licences to track, and still not the font a
  person already uses in other editors.
- **Reading installed fonts through the system's APIs:** C bindings on every platform (ADR-0002).
- **The code style in `zen-ui`:** project settings and the settings importer could not use it.
- **Searching the English texts only:** people who use the IDE in Russian would not find anything.
