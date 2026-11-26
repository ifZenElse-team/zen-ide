# ADR-0107: EditorConfig on save and on open, its status bar mark, and `.editorconfig` files in the IDE

- **Status:** Proposed
- **Date:** 2026-10-22
- **Extends:** [ADR-0100](0100-editorconfig-code-style.md) (the reader, the cache and Enable
  EditorConfig support), [ADR-0097](0097-line-separators-of-documents.md) (documents keep their line
  separator). **Relates to:** ADR-0106 (Editor → General → On Save),
  [ADR-0002](0002-open-source-clean-room-pure-rust.md), [ADR-0033](0033-ai-access-to-every-api.md).

## Context

ADR-0100 made `.editorconfig` a code style layer (indentation, tab width, margins) and left the save
properties (`trim_trailing_whitespace`, `insert_final_newline`, `end_of_line` on save, `charset`),
the status bar's EditorConfig mark, highlighting and completion of `.editorconfig` files, File → New
→ EditorConfig File and Code Style → Show Scheme Actions → Export → EditorConfig File to this
record. What is wanted is known: the properties, the mark and its menu (Open .editorconfig, Show
Files Related to Project…, Disable EditorConfig), the New dialog with property stub check boxes and
the Export action. Open were what the save properties do against Editor → General → On Save, and
whether `end_of_line` converts an existing file on save.

## Decision

1. **One engine for what a save does to whitespace.** Trimming trailing spaces and the final line
   break are Editor → General → On Save's engine (`zen_workbench::on_save`, ADR-0106), which
   also keeps CRLF, caret lines, Markdown hard breaks and multi-line strings. This record adds no
   second one. It gives that engine the EditorConfig side:
   `editor_config::SaveProperties` (`trim_trailing_whitespace`, `insert_final_newline`,
   `end_of_line`, `charset`) from `editor_config::save_properties`, the cached lookup of ADR-0100,
   so that Enable EditorConfig support and the file watcher apply and nothing is read from disk on
   a save.
2. **Precedence: EditorConfig wins over On Save for the files it covers**, `true` and `false`
   alike: a property the file's `.editorconfig` sets replaces the On Save option, one it does not
   set leaves the option (ADR-0106 item 8 says the same). `trim_trailing_whitespace = true` is
   "All lines", `false` is "off"; `insert_final_newline = true` adds the line break, `false` adds
   none but removes none either (the criterion speaks of overriding On Save; removing a line break
   would change files On Save never touches). Remove trailing blank lines has no property and
   follows its option. This order is to be confirmed by the owner.
   **`end_of_line` converts a modified file on save** (`editor_config::before_write`, called by
   `file.document.save` and `file.document.saveAll` after the Actions on Save and before the file
   is written): every line break is rewritten with its separator as one undo step, keeping the
   carets; an unmodified file is not rewritten, and documents opened read-only, images and files
   read-only on disk are left alone.
3. **`charset`** (`charsets::Charset`: `latin1`, `utf-8`, `utf-8-bom`, `utf-16be`, `utf-16le`) is
   the encoding a file is **opened** in and **saved** in, above the project's `.idea/encodings.xml`.
   The five are read and written by the IDE itself (no dependency): Latin-1 maps bytes to U+0000 to
   U+00FF, UTF-16 decodes surrogate pairs and keeps a byte order mark the file had, `utf-8-bom`
   drops the mark from the text and writes it, `utf-8` keeps the bytes as they are (a mark already
   in the file stays). The status bar shows `ISO-8859-1`, `UTF-8`, `UTF-8 BOM`, `UTF-16BE` or
   `UTF-16LE`. Bytes that are not text in the charset (an odd number of bytes in UTF-16, a lone
   surrogate, invalid UTF-8) open read-only with the reason, as ADR-0051's foreign charsets do.
   **A character the charset cannot hold refuses the save**: the file on disk is not touched, the
   document keeps its changes, and the error names the character, its code point, its line and
   column and the charset. Nothing is ever re-encoded silently. Reloading from disk and
   synchronising with it decode in the document's charset. New files (`file.fs.newFile`) take the
   charset of their folder. Line Separators on a closed file and Replace in Files on a closed file
   skip files whose charset is Latin-1 or UTF-16, which they would read as UTF-8; open documents
   are changed in the editor and written in their charset. Local History keeps the saves of a
   document in such a charset as the bytes written (so its versions show as binary), and Revert
   writes those bytes back and reads them into the open document in its charset. Decoding and
   encoding give back the file's bytes exactly, byte order marks included (a second mark after the
   first is text). A `charset` added while a file is open applies when it is opened again.
4. **The status bar's indentation widget** shows the EditorConfig mark (our own icon: a page with
   lines) when `editor_config::style_for` gives the file a style. The marked widget is a button with
   a menu: Open .editorconfig (`project.editorConfig.open`: the nearest file with a matching
   section), Show Files Related to Project… (`view.editorConfig.showFiles`: a dialog listing
   `project.editorConfig.list`, the project's `.editorconfig` files, `(root)` and unreadable lines
   marked; a chosen file opens), and, after a separator, Disable EditorConfig
   (`view.editorConfig.disable`: Enable EditorConfig support off at once and in `settings.json`).
   Without EditorConfig the widget stays a label, as before.
5. **Highlighting and completion of `.editorconfig` files are built into the platform**
   (`editor_config::EditorConfigSyntax`), not a plug-in: the property table they use is the
   reader's (`zen_project::editorconfig::KNOWN`, `known_names`, `known_values`), which validates the
   files; EditorConfig support is a platform feature (ADR-0100). It is
   the last highlighter tried (after the modules' and the plug-ins'), and it answers
   `editor.completion.*` like a plug-in through `PluginFeatures`. Unlike `.gitignore` (the
   `ignore` plug-in, a format of its own), the syntax of `.editorconfig` is the platform's
   EditorConfig support itself (AGENTS.md rule 11: code style and settings are in the platform), and
   a WebAssembly copy of the table would drift from what the reader accepts. Comments, section headers, known
   keys (with `ij_<language>_` and `ij_any_` forms), unknown keys, `=`, numbers, known values and
   other values have colours of their own; before `=` the known keys complete (inserting
   `name = `), after it the values of that key and `unset` (not for `root`).
6. **File → New → EditorConfig File** (`view.editorConfig.new`, in the main menu's new File → New
   submenu, in the Project tool window's New menus and over MCP with `run_action`) opens a dialog
   with three check boxes: `root = true`, the standard properties and the `ij_` ones
   (`ij_continuation_indent_size`, `ij_visual_guides`). OK runs `project.editorConfig.create`,
   which writes the stubs under `[*]` with the values the folder's files get now (so that the code
   style does not change; its `end_of_line` does convert files with other line breaks when they are
   next saved, as item 2 says), opens it, and is refused with code `editorConfigExists`
   when the folder has one. The dialog does not offer stubs per language yet.
7. **Code Style → Show Scheme Actions → Export → EditorConfig File** (a gear next to "Scheme:
   Default (IDE)" on Code Style → General) runs `project.editorConfig.export`: `root = true`, then
   `[*]` with `end_of_line` (unless the line separator is System-Dependent), the indentation,
   `max_line_length` (120 when nothing sets a margin), `tab_width` and the two `ij_` options of
   the default style, then one section per language whose indentation differs, by the globs of its
   files (`code_style::globs_of`). It exports the code style as last applied (the IDE's with the
   project's `.idea` style on top, not EditorConfig's), to `<project>/.editorconfig`; an existing
   file is replaced only after the page's Replace (`overwrite`), with the replaced file kept in Local
   History (action `editorConfigExport`).
8. **For agents** (ADR-0033): `project.editorConfig.get` answers `save` as well;
   `project.editorConfig.open`, `.list`, `.create` and `.export` are commands; the three view
   actions are keymap actions for `run_action`; the `editorConfig` setting stays the switch.

### Joining On Save (ADR-0106)

The On Save change (ADR-0106) adds
`on_save.rs`, called at the end of `format::before_save` and by `format::before_save_all`, and reads
the two EditorConfig properties through its private `editorconfig_properties`
(`zen_project::editorconfig::properties`, from disk on every save). The merge, `on_save` winning
for whitespace:

- `crates/platform/project/src/editorconfig.rs`: keep this branch's file (ADR-0100's reader, a
  superset); drop the On Save branch's copy. `zen_project::editorconfig::properties` exists in both.
- `on_save::options`: replace the lines from `let properties = editorconfig_properties(…)` to
  `let insert_final_newline = …` with `let saved = crate::editor_config::save_properties(session,
  path);`, delete `editorconfig_properties` and the then unused `BTreeMap` import, and use
  `saved.trim_trailing_whitespace` and `saved.insert_final_newline` (the `match` and `unwrap_or_else`
  stay as they are; the two `Options` fields take them too). This replaces ADR-0106's own note on
  an earlier change (`found_for` in `editorconfig_properties`): `save_properties` is that lookup,
  cached and honouring Enable EditorConfig support.
- `format.rs`: a private `after_format(session, path)` runs `crate::on_save::before_save` then
  `crate::editor_config::before_write` and, when both changed the text, `merge_undo_steps(2)` on
  the document, so that one save stays one undo step; `before_save` and the `else` of
  `before_save_all` call it where the On Save branch calls `on_save::before_save`. In `lib.rs`,
  `SaveAll::run` takes the On Save branch's `before_save_all` line and `SaveDocument::run` drops
  this branch's `editor_config::before_write` call; `register` keeps both modules' lines.
- `editor_config::stub_text` (New EditorConfig File) takes `let on_save =
  crate::on_save::options(session, &sample);` and writes `insert_final_newline =
  {on_save.ensure_line_break}` and `trim_trailing_whitespace` as `true` for `Removal::All`, `false`
  for `Removal::Off` and not at all for `Removal::Modified` (the default, which EditorConfig cannot
  say), so that a new file still changes nothing On Save does; `open_list_create_and_export_editorconfig_files`
  then expects no `trim_trailing_whitespace` line in the default stub.
- `end_of_line_converts_a_modified_file_on_save_as_one_step` then expects the first line trimmed
  (EditorConfig's `trim_trailing_whitespace = true` reaches On Save).
- Checked on 2026-10-22 by applying the On Save change onto this one in a scratch
  worktree with the changes above (a merge of the branch itself also brings `main`, whose
  conflicts, `text_replace.rs` among them, come from an earlier change): besides the changelogs and
  documents, the conflicts were `register`, `SaveAll::run`, the settings schema of
  `ide/automation.rs` and one function of `settings/editor.rs` (keep both). Every workbench test
  passed (678) but the On Save branch's
  `random_saves_through_the_editor_keep_every_caret_line_of_every_split_and_undo_in_one_step`,
  which fails the same way without this change (a mixed CR/CRLF text is saved with LF only): that
  is for the On Save merge to settle. The tests show that `true`
  trims all lines and adds a CRLF line break, `false` beats On Save's trimming and final line
  break, a file without EditorConfig gets On Save, Enable EditorConfig support off gives On Save
  back, the whole save (trimming, final line break, `end_of_line`) is one undo step whose result
  Local History keeps, and the stub follows the On Save options.

## Consequences

- A project's `.editorconfig` now decides what a save writes for the files it covers, and in which
  encoding they are read and written; a character that does not fit is reported, never lost.
- Still to be confirmed by hand (the owner): whether `end_of_line` converts an existing file on
  save or only sets new files', whether `insert_final_newline = false` removes a final line break,
  whether `trim_trailing_whitespace = true` keeps Markdown hard breaks and caret lines, the exact
  labels of the New EditorConfig File dialog and its per-language stubs, and the look of the mark. A
  difference is a new ADR.
- Find in Files reads closed files as UTF-8 whatever their `charset`, as before this change.
- Writers of closed files that do not know the charset yet, to be given the same refusal
  (`editor_config::charset` not UTF-8) when they meet this change: language server edits of closed
  files (`language::edits::apply`, rename and intentions; Go and Rust files are UTF-8 by their
  languages' rules, so they reach it only for an ASCII-only Latin-1 file), and the Find in Files
  preview's `search.text.edit` and the Find tool window's replacements (two changes not merged
  yet), which refuse only `.idea/encodings.xml` charsets.

## Alternatives considered

- **A WebAssembly plug-in for `.editorconfig` highlighting:** it could not share the reader's
  property table, would need its own release notes and version, and EditorConfig support is in the
  platform already.
- **An encoding library** (encoding_rs): the five EditorConfig charsets are a few lines each; the
  charsets of `.idea/encodings.xml` beyond them stay read-only (ADR-0051) until a later change adds
  them.
- **Removing the final line break for `insert_final_newline = false`**, as the EditorConfig
  specification words it: it would rewrite files nobody edited that way; left to the owner's check.
