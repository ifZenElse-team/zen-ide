# ADR-0098: Extend Selection from text units and the language server's selection ranges

- **Status:** Proposed
- **Date:** 2026-10-21
- **Relates to:** [ADR-0002](0002-open-source-clean-room-pure-rust.md) (pure Rust),
  [ADR-0003](0003-hybrid-language-intelligence.md) (the syntax layer),
  [ADR-0077](0077-incremental-highlighting-with-line-lexers.md) (line lexers, no syntax tree yet),
  [ADR-0007](0007-everything-is-a-command.md), [ADR-0033](0033-ai-access-to-every-api.md)

## Context

Extend Selection (⌥↑) and Shrink Selection (⌥↓) step a selection through the syntax units around
the caret. The usual way is to
walk a parse tree. We have none: how code is parsed is still open (tree-sitter is C),
ADR-0077 gives us line lexers that know strings and comments
but not structure, and Extend Selection must work in every IDE and every language, plug-ins
included.

## Decision

1. **The units come from the text and its highlighting** (`zen_editor::Structure`, language-free):
   the word at the caret (letters, digits, `_`, and the combining marks that follow them), whole
   even when the caret is inside it; a string's content without and with its quotes; a
   comment block (runs of comment lines) by word, sentence, paragraph and the whole comment; then,
   level by level out through the enclosing brackets (`()`, `[]`, `{}`, skipping those in strings
   and comments): the expression as a chain of names, `.`/`::` links and bracket groups, taken link
   by link from its left; the argument or statement it is in (split at the level's commas,
   semicolons and line breaks; commas count inside `()` and `[]`, and inside braces whose items end
   with commas, as in JSON and struct literals); the bracket's content without and with the
   brackets; at the top, the declaration with the comment and attribute lines right above it, and
   the file. Text and Markdown files step by word, sentence, paragraph and file.
2. **The language server's selection ranges join in** (`textDocument/selectionRange`, gopls and
   rust-analyzer offer it) when the server answers within a short wait (300 ms by default,
   `waitMs`): each step takes the smallest unit or server range that holds the selection and is
   larger, so the server adds what the text cannot see (binary expressions, a multi-line statement)
   and the text adds what the server does not give (string contents, argument lists, comments).
   Where they disagree the server wins: a text unit that overlaps a server range without holding
   it or lying inside it is dropped. An answer that comes after the wait is cancelled and dropped;
   the step is taken from the text alone and never changed afterwards.
3. **One series per editor, kept in the session**: `editor.selection.extend` pushes the selections
   it replaces; `editor.selection.shrink` pops them while the file's text and selections are still
   what the last step left; otherwise it shrinks each selection to the largest unit inside it.
   Every caret steps on its own; overlapping results merge as all selections do.

## Consequences

- Extend Selection works in every file at once, with or without a language server, and needs no
  parser; a parser chosen later can replace `Structure`'s heuristics language by language.
- The wait for the server is part of the step: in the window, a server that is slow to answer
  holds that one frame for up to `waitMs` (300 ms); the units themselves are worked out only when
  a step is asked for, never while the editor is idle.
- Without a server, a statement split over lines without brackets or an operator at the break is
  taken line by line, and binary expressions are not a step.
- Shrink after a selection made by hand, and several carets, are specified provisionally;
  both are marked [check] in the UI specification until the owner confirms them.

## Alternatives considered

- **Tree-sitter.** C, against ADR-0002; the parsing decision is not made.
- **The language server only.** No strings' contents or argument lists from gopls, nothing without
  a server, a wait on every press.
