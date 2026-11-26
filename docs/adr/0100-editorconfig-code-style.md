# ADR-0100: EditorConfig is a per-file code style layer above the project's

- **Status:** Proposed
- **Date:** 2026-10-22
- **Extends:** [ADR-0053](0053-fonts-ui-scale-and-code-style.md) (its item 5 deferred
  `.editorconfig`), [ADR-0097](0097-line-separators-of-documents.md) (which read `end_of_line`
  only). **Relates to:** ADR-0094 (the order of the indentation sources; not merged yet),
  [ADR-0033](0033-ai-access-to-every-api.md).

## Context

The requirement: the IDE reads the project's `.editorconfig` files
(<https://spec.editorconfig.org>), on by default (Settings → Editor → Code Style → General → Enable
EditorConfig support). A property a file sets for a path wins over the Code Style scheme (the
project's or the IDE's); a property it does not set comes from the scheme. Besides the standard
properties it reads the `ij_` ones: `ij_<option>` for every language, `ij_any_<option>` for many,
`ij_<language>_<option>` for one; the language's beats `ij_any_` beats the general one.

Our code style is per language (`CodeStyle`, the user's Editor → Code Style, with the project's
`.idea/codeStyles` on top through `CodeStyleOverride`), and `zen_project::editorconfig` (ADR-0097)
read only `end_of_line`, straight from disk on every use.

## Decision

1. **Reading.** `zen_project::editorconfig::File::parse` reads a file once into its `root` flag and
   its sections; lines it cannot read are `problems` (by line): a line that is neither a section, a
   property nor a comment, an unclosed `[`, a property other than `root` before the first section, a
   property without a name, and a value a known property does not take (`indent_style`,
   `indent_size`, `tab_width`, `end_of_line`, `charset`, `trim_trailing_whitespace`,
   `insert_final_newline`, `max_line_length`, `root` and the `ij_` options below). A line that
   cannot be read is skipped, and the properties under an unclosed section header apply to nothing.
   Unknown keys are kept and ignored; `root` inside a section is ignored. A UTF-8 byte order mark
   is skipped, CRLF line ends are read, and only a line that starts with `#` or `;` is a comment
   (as in the specification, a `#` or `;` after a value is part of it). `lookup` walks from the file's folder up to the project's root, or to the
   first file with `root = true`; the nearest file wins, a later section wins within a file, and
   `unset` drops a property. Globs: `*`, `**`, `?`, `[...]`, `[!...]`, `{a,b}` (nested), `{1..3}`
   (negative bounds too; a number written with a leading zero does not match), `\` escapes (also
   inside brackets), `/**/` for zero or more folders; brackets holding a `/` are plain characters; a
   glob with a `/` outside brackets matches from the file's folder, one without matches the name in
   any folder. A backslash is always an escape, never a Windows folder separator. Matching keeps the
   set of positions each part of the glob can reach, so its time grows with the length of the glob
   times the square of the path's at most, whatever the stars and braces, and it does not recurse
   per character.
2. **Mapping** (`editorconfig::Style::from_properties`): `indent_style` → tabs or spaces;
   `indent_size` → the indent, `tab` making it the tab size; `tab_width` → the tab size;
   `max_line_length` → the right margin, `off` → none; `ij_continuation_indent_size` and
   `ij_visual_guides` (columns separated by commas, `none` or empty for none), each looked up as
   `ij_<language>_` (a small table of language ids: `go`, `rust`, `json`, `yaml`, ...), then
   `ij_any_`, then `ij_`. As the specification says, `tab_width` defaults to a numeric `indent_size`
   and `indent_size` to `tab` under `indent_style = tab`.
3. **The layers**, lowest first: the built-in style of the language, the user's Editor → Code
   Style, the project's own code style (`.idea/codeStyles`), EditorConfig. `FileCodeStyle` is the
   whole style of one file (indentation, tab size, continuation indent, right margin, visual guides);
   `project_settings::StyleLayers` holds the two project layers of a path and applies them in that
   order. Everything that indents goes through it: Tab, Enter and ⇧Tab and live templates
   (`indent_unit_of`), the status bar's indent, the width of a tab in the editor
   (`code_style_for_path`) and Reformat Code's `tabSize`/`insertSpaces`.
4. **Caching.** The workbench keeps each folder's parsed `.editorconfig` (`editor_config::EditorConfigs`),
   a missing file included, and what the files give each path asked about. Enter, Tab and the
   editor's frames are answered from that cache without touching the disk. The project's file
   watcher (`project.settings.refresh` with the changed paths) forgets the cache when an
   `.editorconfig` changed, so editing, adding or deleting one applies to open files at once,
   without reopening them. As a safety net for changes the watcher does not report (an excluded
   folder, a watcher that could not start), a lookup more than a second after the last check
   compares the size and modification time of every cached folder's file once and drops what
   changed. `project.settings.get` checks the disk every time.
5. **Problems and sources.** `project.settings.get` adds, to the problems and sources the providers
   read, every `.editorconfig` file of the project the file index lists and every one read for a
   file, with their problems as `Line <n>: <message>`.
6. **The switch.** `IdeSettings.editorConfig` (on by default; Settings → Editor → Code Style →
   General → Enable EditorConfig support) is the session setting `editor.editorConfig.enabled`. Off,
   every `.editorconfig` file is ignored: no code style, no `end_of_line` for new files and Replace in
   Files, no problems.
7. **For agents**, `project.editorConfig.get { path? }` answers what the files give a file: the
   matching files (nearest first), the properties, the `Style` they set and the resulting
   `FileCodeStyle`.

### Joining indent detection (ADR-0094)

ADR-0094 reserves "EditorConfig, when it comes" between the project's
style and detection, and a `fixed` flag on `file_indent::ConfiguredStyle` that EditorConfig sets
when it gives the indentation. Its `configured_at` already builds the configured style with
`project_settings::code_style_for_path`, which includes the EditorConfig layer here, and its Reformat
Code uses that configured style. Joining the two is one change in `file_indent::configured_at`:

```rust
ConfiguredStyle {
    style: project_settings::code_style_for_path(session, path),
    fixed: crate::editor_config::style_for(session, path).is_some_and(|style| style.sets_indent()),
}
```

`Style::sets_indent` is true when `indent_style` or `indent_size` is set for the file, so detection
then yields to EditorConfig; a file whose `.editorconfig` sets only `tab_width` or
`max_line_length` is still detected. The change is the value of the one field `fixed` (rustfmt
then puts the struct literal on four lines). Checked on 2026-10-22 by merging
the indent-detection change (ADR-0094) into this one: without it an `.editorconfig` with
`indent_style = tab` lost to a file indented with spaces; with it the workbench's EditorConfig,
detection, format and project settings tests passed. The merge conflicts besides the changelogs:
both branches add a check box to Code Style → General, a field to `IdeSettings` and lines to the
same guide pages and locales (keep both); in `language/format.rs` take the detection branch's
`configured_style` (it reads `configured_at`, which includes EditorConfig) and drop `StyleLayers`
there, keeping this branch's Reformat Code test; in `Document`'s text update call both
`detect_line_separator` and `detect_indent`.

### Joining the visual guides

`origin/feat/visual-aids` draws the right margin and the visual guides from
`visual_aids::margins_of`, which reads the project's `.idea` code style (`RIGHT_MARGIN`,
`SOFT_MARGINS`) and falls back to 120. `editor_config::margins_over` puts `max_line_length` and
`ij_visual_guides` on top of such a pair, so joining them is one more statement at the end of
`margins_of`:

```rust
let margins = (project.right_margin.unwrap_or(DEFAULT_RIGHT_MARGIN), project.visual_guides.unwrap_or_default());
crate::editor_config::margins_over(session, path, margins)
```

`max_line_length = off` leaves the drawn margin to the project and the IDE, as EditorConfig defines
`off` (the editor's own setting); `project.editorConfig.get` reports it as no right margin. Checked
the same day by merging the visual-aids change: it compiles with this change, and an
`.editorconfig` with `max_line_length = 100` and `ij_visual_guides = 72` makes
`visual_aids::shown` draw 72 and 100 (120 with EditorConfig support off). The same merge needs
`StyleLayers::apply_to_file` to borrow the project layer (done here), because that branch makes
`CodeStyleOverride` hold a `Vec` and no longer `Copy`; for `FileCodeStyle` (and
`project.editorConfig.get`) to report the `.idea` visual guides too, `CodeStyleOverride::apply_to_file`
then takes `visual_guides: self.visual_guides.clone().unwrap_or(base.visual_guides)`.

## Consequences

- A project's `.editorconfig` now sets indentation, tab width and right margin;
  the right margin, continuation indent and visual guides are reported (`project.editorConfig.get`)
  and drawn once the visual guides are joined as above.
- A keystroke asks the cache only: in a project of 441 `.editorconfig` files, a file 41 folders
  deep took 0.73 ms for its indent and tab size per keystroke when every folder's file was checked
  on disk, 0.015 ms from the cache (debug build, [performance](../performance.md#editorconfig-on-every-keystroke)).
- The save properties (`trim_trailing_whitespace`, `insert_final_newline`, `charset`), the status
  bar's EditorConfig mark and menu, highlighting of `.editorconfig` files and File → New →
  EditorConfig File are the next work.

## Alternatives considered

- **A settings provider that walks the project for `.editorconfig` files on open:** it would read
  every folder of a large project before the first file is shown, and still need a per-path lookup.
- **Merging EditorConfig into `ProjectSettings.code_style`:** that map is per language; EditorConfig
  is per path and glob.
- **Reading the files from disk on every use** (as ADR-0097 did): the status bar and the editor ask
  every frame.
- **Checking each file's size and time on every use:** one `stat` per folder between the file and
  the root on every keystroke and frame; the watcher already reports the changes.
