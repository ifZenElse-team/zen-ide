# ADR-0099: Unwrap/Remove from brackets and keywords, without a parser

- **Status:** Proposed
- **Date:** 2026-10-22
- **Relates to:** [ADR-0002](0002-open-source-clean-room-pure-rust.md) (pure Rust),
  [ADR-0003](0003-hybrid-language-intelligence.md) (the syntax layer),
  [ADR-0077](0077-incremental-highlighting-with-line-lexers.md) (line lexers, no syntax tree yet),
  [ADR-0007](0007-everything-is-a-command.md), [ADR-0033](0033-ai-access-to-every-api.md),
  [ADR-0038](0038-keymap.md), [ADR-0039](0039-main-menu.md), ADR-0098 (Extend Selection without a
  parser)

## Context

Code → Unwrap/Remove… (⇧⌘⌦) lists the constructs around the caret and takes one away, keeping or
dropping its body. Such an action is usually read from a parse
tree. We have none: how code is parsed is still open, ADR-0077 gives us line lexers that know
strings and comments but not structure, gopls and rust-analyzer offer no such action, and it must
work in every IDE and language. Extend Selection faces the same gap and answers it
with text units in `zen_editor`.

## Decision

1. **The constructs come from the text** (`zen_editor::unwrap_options`, language-free): brackets
   are paired skipping the strings and comments the file's highlighter knows; each `{` block is
   named by the words before it on its line (back to a line break, `;`, or an unmatched bracket;
   past `;` when that reaches `if`, `for`, `switch` or `else`, for Go's init statements): `if` and
   the `else`/`else if` blocks after it form a chain; `for`, `while`, `loop` are loops; `switch` and
   `select` blocks are split into their `case`/`default` clauses; a nested `func`/`function` with a
   parameter list right before the block is a function literal; a block with nothing before it is
   bare braces. A `(…)` group after a name with one argument (a trailing comma allowed) is a call,
   unless it is a pattern or cannot stand alone: after `let`, `go`, `defer`, `struct`, `enum` or
   `impl`, before `=>`, or in an attribute `#[…]`. A function literal is offered only when the
   statement holding it is whole on its first and last lines: every bracket there pairs with one
   there too, and no `;` but a final one.
2. **The entries**, innermost first: `Unwrap 'if…'` (the chain goes, the first body stays),
   `Unwrap 'else…'` (the chain goes, that branch's body stays), `Remove 'else…'` (the branch goes),
   `Unwrap 'for…'`, `Unwrap 'case…'` (the `switch` goes, the clause's statements stay),
   `Remove 'case…'` (the clause goes), `Unwrap 'func…'` (the lines from the statement holding the
   literal to its closing brace become the body), `Unwrap braces`, `Unwrap 'f(…)'` (the call
   becomes its argument). A body is re-indented to the line the construct started on, keeping its
   inner indentation; lines that start inside a string or comment (Go raw strings, Rust raw
   strings, block comments) stay exactly as they are. A construct that has whole lines replaces
   them, and an empty body removes them. Everything that goes is bracket-balanced, so the edit
   never takes half of a neighbouring construct.
3. **Commands and the window.** `editor.unwrap.list` and `editor.unwrap.apply` (zen-workbench) list
   and apply the entries, one undo step each; `view.unwrap.open` (⇧⌘⌦, Code menu, `Unwrap` in an
   imported JetBrains keymap) applies a single entry at once and otherwise opens the popup, which
   highlights the code that stays.

## Consequences

- Unwrap/Remove works in every brace language at once, with or without a language server; a parser
  chosen later can replace the heuristics language by language behind the same commands.
- What is not brace-shaped is not offered: Python and YAML blocks, Rust `match` arms, closures
  without a `func`/`function` keyword, multi-line headers broken without a bracket.
- ADR-0098 (`zen_editor::Structure`, Extend Selection) pairs brackets and skips
  strings and comments the same way. The two are separate decisions about different actions, so
  both records stay; the shared part is one mechanism, and whichever merges second moves its
  bracket pairing onto the other's, so that `zen_editor` has one parser-free bracket matcher.
- The Go list, the entry names and whether one entry runs without the popup are marked [check] in
  the UI specification until the owner confirms them.

## Alternatives considered

- **Tree-sitter.** C, against ADR-0002; the parsing decision is not made.
- **The language server.** Neither gopls nor rust-analyzer offers it as a code action, and nothing
  would work without a server.
