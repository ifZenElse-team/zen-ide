# ADR-0101: Custom folding regions from comments, and manual regions kept by the document

- **Status:** Proposed
- **Date:** 2026-10-22
- **Relates to:** [ADR-0007](0007-everything-is-a-command.md),
  [ADR-0033](0033-ai-access-to-every-api.md), [ADR-0056](0056-plugin-abi-v1.md),
  [ADR-0077](0077-incremental-highlighting-with-line-lexers.md), ADR-0092 (Surround With as live
  templates), [ADR-0096](0096-two-keystroke-shortcuts-and-the-keypad-star.md)

## Context

The editor is to fold two kinds of region the code itself does not show: **custom regions**, marked
by comments (`// region Name` … `// endregion`, `//<editor-fold desc="Name">` … `//</editor-fold>`)
in each language's comment syntax and folded to their name, and **manual regions** made with Fold
Selection / Remove Region (⌘.), which fold any selected text, part of a line too. Custom Folding…
(⌥⌘.) lists the custom regions for going to one. Both kinds are remembered with the file's other
folds.

Our folds came from three places (a plug-in's line ranges, a lexer's tokens for Go, or the brackets
of the text), were kinds `block`, `imports` and `comment`, always spanned lines, and were
remembered as whole lines. While typing goes on the editor moves the folds last read along with
the edits and reads them again only once the text settles (`Document::foldable_listed`, an earlier
change); that must stay so.

## Decision

1. **Custom regions are read from the text, in the file's comment syntax**
   (`zen_editor::custom_regions`, with `zen_syntax::comment_syntax`): a line that holds nothing but
   one comment opens or closes a region: `region Name` / `endregion` (also `#region`), or
   `<editor-fold desc="..." defaultstate="...">` / `</editor-fold>`; a line comment where the
   language has one, else a block comment closed on the same line. A closing comment pairs with the
   innermost open region written the same way; unpaired ones fold nothing. A region folds from its
   opening comment to the end of its closing one. A region comment inside a string or a block
   comment does not count: where the file has a highlighter, its tokens tell (the comment must be a
   comment token that starts there, on a line that does not carry a string or a comment over from
   the line before); elsewhere a plain scan of the text skips block comments, quoted strings and
   triple-quoted strings (`zen_editor::custom_regions_where`, `zen_editor::custom_regions`).
2. **They join every source of folds.** Whatever gave the folds (plug-in, lexer, brackets), the
   custom regions are added as `FoldKind::Region` (`zen_editor::with_custom_regions`), and runs of
   line comments that hold a region comment are dropped, as the region folds those lines.
   `editor.fold.list` names the kind `region` and gives the name as `placeholder`, worked out for
   the listed folds only; the editor draws the name as the fold's mark. `defaultstate="collapsed"`
   folds a region when a file opens without remembered folds.
3. **Manual regions are document state.** `Document` keeps them as character ranges and moves them
   with every edit by what they hold (`CharEdit::map_region`): the characters still there and what
   is put between them, never what is put at their edges, so text typed or pasted right before or
   after one, or over its first or last characters, stays out; a replacement is first narrowed to
   what it really changes (an external reload changes only where the file did), and a region goes
   once none of its characters are left. The document adds them to the folds it lists
   as `FoldKind::Manual` (`manual`), also to the folds last read, so that adding or removing one
   needs no new read and typing keeps moving them without reading the text. A fold may now start and
   end on the same line; the editor draws the rest of the line after its mark and places carets,
   selections, highlights and clicks after it accordingly.
4. **Remembered with lines and columns.** `editor.fold.remembered` / `editor.fold.remember`
   (`FileFolds`, the workspace's `folds`) gain `manual`: each manual region's start and end line and
   column (characters, from 0), whether it is folded and `textHash`, a 32-bit FNV-1a hash of the
   text it holds; files saved before have none (`#[serde(default)]`). A manual region comes back
   where the file still has both places and, when the hash is there, the same text between them, so
   one whose text changed on disk while the file was closed is dropped rather than put on other
   text. The other folds stay whole lines.
5. **Commands** (ADR-0007, ADR-0033): `editor.fold.selection` (Fold Selection / Remove Region, ⌘.,
   Code → Folding) folds every selection as a manual region (a selection that ends at a line's
   start stops at the end of the line before) or, without one, removes the innermost manual region
   on the caret's line or around it; `editor.fold.regions` lists the custom regions (name, start,
   end, line from 0, depth, style, folded). The window action `view.customFolding.open` (⌥⌘.,
   Navigate) shows them in a popup at the caret and answers with them; choosing one runs
   `navigate.place.goTo` at its start. The JetBrains keymap import maps `CollapseSelection` and
   `GotoCustomRegion`.

## Consequences

- Custom regions work in every language with a comment syntax, with no work per language, and in
  plug-in files too; Surround With's region surrounders (ADR-0092) produce what is read here.
- Reading the folds once the text settles goes through the lines once more (a check of each line's
  start): measured in a debug build on a 2 200-line file, 15.1 ms instead of 13.8 ms; typing moves
  the regions without reading, 1.4 ms a keystroke frame with 100 regions instead of 1.3 ms. In a
  release build (review, 2026-10-22): 0.08 ms a keystroke frame instead of 0.07 ms, and 0.84 ms
  instead of 0.66 ms to read the folds once the text settles.
- A region comment inside a string or a block comment is skipped with the file's highlighter
  where it has one; without one, the plain scan does not know a language's other multi-line strings
  (Go's raw strings and JavaScript's template literals have highlighters).
- A same-line fold is drawn on its line only: a second fold folded on that line is not drawn folded
  (its lines stay in view, so no text is hidden without a mark), until the first one opens.

## Alternatives considered

- **Each language finds its region comments** (in its lexer or plug-in): exact about strings and
  comments, but every language would need the work, and plug-ins would need a new export.
- **Manual regions as a separate list the view merges**: the editor's fold listing and every
  `editor.fold.*` command would have to merge it themselves; keeping them in the document gives one
  list for all of them.
- **Remember manual regions as whole lines**: loses a region inside a line, which Fold Selection
  makes often.
