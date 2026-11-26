# ADR-0106: What a save does to whitespace: modified lines since the last save, caret lines, significant spaces and EditorConfig

- **Status:** Proposed
- **Date:** 2026-10-22
- **Relates to:** [ADR-0002](0002-open-source-clean-room-pure-rust.md) (clean-room),
  [ADR-0033](0033-ai-access-to-every-api.md) (AI access)

## Context

The UI specification gives Settings → Editor → General an On Save section: **Remove trailing spaces on:
[Modified lines | All lines]**, **Keep trailing spaces on caret line**, **Remove trailing blank
lines at the end of saved files** and **Ensure every saved file ends with a line break**.
Significant trailing spaces stay. What "modified lines" are compared with,
the exact defaults, and whether EditorConfig's `trim_trailing_whitespace` and
`insert_final_newline` beat the section were not specified there. Zen IDE had no On Save section;
a save ran only the Actions on Save (Optimize
Imports, then Reformat Code, `zen_workbench::language::format::before_save`).

## Decision

1. **Where.** `zen_workbench::on_save` runs inside `format::before_save`, after the Actions on Save
   and before the file is written, so every path that saves through it (Save, Save All, the save of
   `editor.text.replace`, exit) applies the options; the commit's save and an inspection fix's save
   call it too. Save All runs it for every file even after a language server timed out
   (`format::before_save_all`), since it never waits.
2. **The settings** are per IDE (`settings.json`, `set_settings`): `removeTrailingSpaces` (on),
   `removeTrailingSpacesOn` (`modified` or `all`; `modified`), `keepTrailingSpacesOnCaretLine`
   (on), `removeTrailingBlankLines` (off), `ensureLineBreakAtEnd` (off), as the session settings
   `editor.onSave.*`. The defaults are those the owner asked for.
3. **Modified lines are the lines that differ from the text last saved or read from disk**: a line
   diff of the document's saved text and its text, so a line changed and changed back is not
   modified, and a line removed modifies no other line. Lines are split at LF, CRLF and CR alike
   and compared without their line break, so a CR-only or mixed file is diffed line by line and a
   line whose separator alone changed is not modified. The diff is Myers'
   (`zen_vcs::diff::changed_lines`), not the gutter's histogram diff (`hunks`): on a 100 000-line
   file with every other line changed the histogram diff takes 7.6 s in a release build, Myers'
   4 ms. Where lines move, a line diff has to choose which lines it keeps in common, and a line it
   does not keep counts as modified even if nobody typed in it. Not the lines
   changed against version control.
4. **Caret lines.** With Keep trailing spaces on caret line, every line holding a caret of the
   document, in every editor group showing it, keeps its trailing spaces, in both modes; trailing
   blank lines go only below the last caret line, so a save never moves the caret's line.
5. **Significant spaces stay**: in Markdown files (`.md`, `.markdown`, `.mdown`, `.mkd`), two or
   more spaces after text (a hard line break); in every language, trailing spaces that start inside
   a string literal by the file's highlighting (the language's own or a plug-in's), which is what a
   string literal running on to the next line looks like (Go raw strings, Rust and JavaScript
   multi-line strings). Only spaces and tabs count as trailing whitespace.
6. **Line separators stay.** Lines end at LF, CRLF or CR; only what precedes the line break
   changes, and a line break added at the end is the file's first one (LF when it has none), so a
   CRLF file stays CRLF. With ADR-0097 (documents keep their separator) this composes: the inserted
   break is converted to the document's separator as every edit is.
   **Separators never merge.** A line of only spaces and tabs that follows a lone CR and ends with
   LF (`"a\r  \nb"`) keeps the first of them (`"a\r \nb"`): emptied, it would put the CR next to
   the LF, which then read as one CRLF separator, and the line would vanish. Such a line must
   stay a line, so the least Zen IDE can leave while keeping each separator as it is is one
   whitespace character, not the whole run: rewriting a separator would break this decision, and keeping all
   the spaces would leave more than is needed. The other choices do not merge separators: a line
   after CRLF or LF, a line ending with CR or CRLF, or the last line empties as usual, and
   Remove trailing blank lines cuts from the end of the last line kept to the end of the file, so
   what precedes the cut always ends with its own whole separator.
7. **One undo step, before writing.** All the changes are one `Document::replace_ranges` call; a
   caret at the end of the file stays before a line break added there. Unmodified documents, images,
   documents opened read-only and files that are read-only on disk are left alone. A replacement
   of many ranges updates the document's text and moves its places (bookmarks, folds, carets of
   other editor groups) in one pass, not one pass per range: saving a 100 000-line file with every
   other line modified takes 30 ms for the cleanup in a release build (50 ms with All lines),
   where one pass per range took seconds for 10 000 lines (docs/performance.md).
8. **EditorConfig wins** for the files of the project its sections match: `trim_trailing_whitespace
   = true` removes trailing spaces from all lines, `false` from none; `insert_final_newline` turns
   the final line break on or off. Remove trailing blank lines has no EditorConfig property and
   follows the setting. The properties come from one private function,
   `on_save::editorconfig_properties`, which reads them with `zen_project::editorconfig::properties`
   (a copy of ADR-0097's reader, the same bytes, from the file's folder up to the project's folder
   or a `root = true` file) and gives nothing for a file outside the project, as ADR-0100's
   `editor_config::found_for` does; that function is what changes when ADR-0100's cache arrives
   (see Merging).
9. **For agents**, `editor.onSave.options { path? }` answers what applies to a file after
   EditorConfig, and the save commands' descriptions name the step.

## Merging

Checked with `git merge-tree` on 2026-10-22 against `origin/feat/line-separators` (ADR-0097),
`origin/feat/editorconfig` (ADR-0100) and `origin/fix/close-tab-keeps-edits`. The
conflicts below are the ones this change adds; those the branches already have with `main`
(`text_replace.rs`, `docs/README.md`, `docs/automation.md`, the product changelogs) are theirs.

- **`feat/line-separators`**: `crates/platform/project/src/editorconfig.rs` is the same file in
  both and merges cleanly. Both sides add lines next to each other in `CHANGELOG.md`,
  `docs/README.md` and `docs/automation.md` (keep both), `ui/src/ide/automation.rs` (both
  settings schemas), `ui/src/ide/settings/editor.rs` (both groups and imports) and
  `workbench/src/lib.rs` (`on_save::register` and `line_separators::register`, keep both).
  ADR-0097's `Document::replace_ranges` writes the line breaks it inserts with the document's
  separator, so the random editor test opens the file with its mixed text and sets the saved text
  beside it instead of planting the text with an edit; an edit would turn every CR and CRLF of a
  file that starts with LF into LF, and the oracle, which keeps them, would fail.
- **`feat/editorconfig`**: the same, plus `editorconfig.rs` as an add/add conflict, where
  that branch's file is taken whole (ADR-0097's reader with the cache's additions;
  `zen_project::editorconfig::properties` keeps its signature). Then `on_save::editorconfig_properties` uses the cache:
  `crate::editor_config::found_for(session, path).map(|found| found.properties).unwrap_or_default()`,
  which also honours Enable EditorConfig support (`editor.editorConfig.enabled`). When ADR-0107's
  change lands on top of that branch, its `editor_config::save_properties` replaces the function
  and the `flag` closure in `on_save::options`, as ADR-0107 describes.
- **`fix/close-tab-keeps-edits`**: one conflict in `workbench/src/lib.rs`. In
  `SaveAll::run` take that branch's body (`closing::save_documents`, `closing::failure`); in
  `SaveAll::DESCRIPTION` join both: "Saves every modified file, first optimizing imports and
  reformatting when editor.onSave.* say so, then applying the On Save options
  (editor.onSave.options) to each, as one undo step per file. A file that cannot be saved keeps
  its changes and the error names it; the others are saved." `closing.rs` merges without a
  conflict but must change too: in `closing::save_documents` the loop that calls
  `language::format::before_save` and breaks at the first timeout becomes
  `language::format::before_save_all(session, &dirty);`. Without it the test
  `a_save_all_stops_formatting_once_the_server_does_not_answer_but_cleans_every_file` fails; with
  it all 647 `zen-workbench` tests and the headless MCP tests pass on the trial merge.

## Consequences

- Saving now changes files by default (trailing spaces of the modified lines).
- What "modified lines" are compared with (the last save or VCS), the defaults,
  the caret line and trailing blank lines together, and EditorConfig's precedence are
  to be confirmed by the owner; a difference is a new ADR.
- A file whose highlighter cannot tell strings has no protected strings (Python has no highlighter
  in Zen IDE yet, so its triple-quoted strings lose their trailing spaces with All lines); a
  language module that is off in an IDE (Go in RustRonin, Rust in GoGarden) highlights nothing
  there. Markdown hard breaks are known by the file name.
