# ADR-0119: Surround With puts code into live templates; language modules declare surrounders as data

- **Status:** Proposed
- **Date:** 2026-10-22
- **Relates to:** [ADR-0007](0007-everything-is-a-command.md), [ADR-0038](0038-keymap.md),
  [ADR-0039](0039-main-menu.md), [ADR-0051](0051-jetbrains-keymaps-and-idea-project-settings.md),
  [ADR-0071](0071-built-in-modules.md), [ADR-0077](0077-incremental-highlighting-with-line-lexers.md),
  ADR-0098 (Extend Selection without a parser), ADR-0099 (Unwrap/Remove
  without a parser)

## Context

Surround With (⌥⌘T) wraps code in a construct, and Surround with Live Template (⌥⌘J) wraps it in
a live template. Each language has its own constructs (Go: `if`,
`if / else`, `for`, a func literal, `(expr)`, `!(expr)`), every language gets the custom folding
region comments, and live templates mark where the code goes with `$SELECTION$`. We already have
live templates with variables, `$END$`, indentation and imports (`zen_workbench::templates`), and
no parser that knows statements in every language.

## Decision

1. **A surrounder is a live template.** Its text uses the template syntax with `$SELECTION$` for
   the wrapped code; applying one inserts it like a template (one undo step, indented as the line,
   variables filled in with Tab, the caret at `$END$`). `$SELECTION$` is a reserved variable in
   every template: empty when a template is expanded by its abbreviation, the wrapped code under
   Surround With. The wrapped lines lose the indentation of the first one's line and take that of
   the template line `$SELECTION$` is on.
2. **Language modules declare surrounders as data**, next to their templates:
   `TemplateSet.surrounders`, the JSON shape of `zen_workbench::templates::Surrounder` (`language`,
   `id`, `title` as a message id, `text`, `variables`, `expression`). The platform adds
   `<editor-fold…>` and `region…endregion` comments to every file with a comment syntax
   (`zen_syntax::comment_syntax`). User templates that hold `$SELECTION$` come after them.
3. **What is wrapped is found from the text, without a parser** (`templates::surround::wrap`,
   language-free): brackets are paired skipping the strings and comments the file's highlighter
   knows (ADR-0077; a highlighter's per-line pieces of one string or comment are joined). Without
   a selection, the statement at the caret: its line without the indentation, down to the line
   where the brackets it opens close and past lines that end with a binary operator, `,` or `.` or
   whose next line starts with `.`, `&&`, `||`, `?` or `:`; a caret inside a string or comment
   that started on an earlier line, inside `(` or `[`, inside a `{` whose items end with commas (a
   literal), on a line that closes a bracket opened earlier (`}`, `} else {`, `})`) or after a
   continued line goes out to the statement that holds it; a blank line wraps nothing. A selection
   grows to the whole statements it touches, then until every bracket in it is paired, for a
   statement surrounder; it is taken as it is for an expression one (`expression: true`, or a
   template whose `$SELECTION$` is not alone on its line). Expression surrounders are offered only
   for a selection that holds code. Lines that start inside a string or comment (Go and Rust raw
   strings, block comments) keep their bytes, and the template's own lines take the file's line
   breaks (CRLF stays CRLF).
4. **Two commands and two window actions** (ADR-0007): `editor.surround.list` (items with their
   mnemonic, the wrapped range and, on request, a preview) and `editor.surround.apply`; the popup
   is `view.surround.open` (⌥⌘T, Code menu) and `view.surroundWithTemplate.open` (⌥⌘J, a new keymap
   group Other); the JetBrains keymap import maps `SurroundWith`
   and `SurroundWithLiveTemplate`.

## Consequences

- A language gets Surround With by shipping a JSON list; a plug-in language can get it the same way
  once plug-ins declare templates.
- A statement split over lines without a bracket, an operator or a leading `.`, `&&`, `||`, `?`
  or `:` at the break is wrapped one line at a time unless selected; a block's statements that end
  with a comma (JavaScript's `let a = 1,`) make it read as a literal.
- ADR-0098 (`zen_editor::Structure`) and ADR-0099 (`zen_editor::unwrap_options`)
  pair brackets outside strings and comments without a parser too, and
  ADR-0099 also keeps lines that start in a string or comment. All three are Proposed and in
  separate branches; they are separate decisions about different actions, so the records stay,
  but the bracket pairing is one mechanism: whichever of these branches merges after another
  moves its pairing onto the one already in `main`, so that the platform keeps one parser-free
  bracket matcher.
- A language server could later give the statement or expression ranges (LSP selection ranges) without changing the commands.

## Alternatives considered

- **Surrounders as code per language** (a trait in the module API): more power (checking that
  `!(expr)` wraps a boolean) for every language, but each would need its own parser; data covers
  the Go constructs listed above.
- **Surrounders as built-in live templates**: they would show in Settings → Live Templates and
  expand by abbreviation, which surrounders do not do.
